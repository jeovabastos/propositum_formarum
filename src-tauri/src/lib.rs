use image::codecs::jpeg::JpegEncoder;
use kmeans_colors::get_kmeans;
use palette::{LinSrgb, Srgb};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(target_os = "linux")]
use std::process::Command;

// ---------------------------------------------------------------------------
// Estado e modelos
// ---------------------------------------------------------------------------

pub struct AppState {
    pub db: SqlitePool,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ImageEntry {
    pub id: i32,
    pub name: String,
    pub path: String,
    pub notes: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct FolderEntry {
    pub folder: String,
    pub count: i64,
}

#[derive(Serialize, Deserialize)]
pub struct MipmapPaths {
    original: String,
    mid: String,
    thumb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Moodboard {
    pub id: i64,
    pub name: String,
    pub x_camera: f64,
    pub y_camera: f64,
    pub zoom_camera: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemImageProperties {
    pub id_image_catalog: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDrawingProperties {
    pub path_string: String,
    pub color: String,
    pub thickness: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width_original: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_original: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "unique_type")]
pub enum MoodboardItem {
    #[serde(rename = "image")]
    Image {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        properties: ItemImageProperties,
    },
    #[serde(rename = "drawing")]
    Drawing {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        properties: ItemDrawingProperties,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SalvarMoodboardDTO {
    pub id: Option<i64>,
    pub name: String,
    pub x_camera: f64,
    pub y_camera: f64,
    pub zoom_camera: f64,
    pub items: Vec<MoodboardItem>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Hash estável do caminho completo, usado nos nomes de miniaturas.
/// (Nome do arquivo sozinho colide quando há imagens iguais em pastas diferentes.)
fn short_hash(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().as_str()[..16].to_string()
}

/// Pasta (diretório pai) de um caminho de arquivo.
fn folder_of(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Contador para nomes únicos de arquivos temporários de miniatura.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const IMAGE_EXTS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map_or(false, |ext| IMAGE_EXTS.iter().any(|x| ext.eq_ignore_ascii_case(x)))
}

// ---------------------------------------------------------------------------
// Janela
// ---------------------------------------------------------------------------

#[tauri::command]
async fn close_splashscreen(app_handle: AppHandle) {
    if let Some(main_window) = app_handle.get_webview_window("main") {
        let _ = main_window.show();
        let _ = main_window.set_focus();
    } else {
        println!("ERRO: Janela 'main' não foi encontrada!");
    }

    if let Some(splash_window) = app_handle.get_webview_window("splashscreen") {
        let _ = splash_window.close();
    } else {
        println!("Aviso: Janela 'splashscreen' não foi encontrada ou já estava fechada.");
    }
}

// ---------------------------------------------------------------------------
// Indexação
// ---------------------------------------------------------------------------

/// (nome, caminho, pasta)
type ItemBatch = (String, String, String);

/// Grava um lote em UMA transação. Divide em blocos de 300 linhas
/// (3 colunas x 300 = 900 variáveis, abaixo do limite antigo de 999 do SQLite).
async fn save_batch_to_database(lote: &[ItemBatch], db: &SqlitePool) -> Result<usize, String> {
    if lote.is_empty() {
        return Ok(0);
    }

    let mut tx = db.begin().await.map_err(|e| e.to_string())?;
    let mut inserted = 0usize;

    for chunk in lote.chunks(300) {
        let mut qb =
            QueryBuilder::<Sqlite>::new("INSERT OR IGNORE INTO images (name, path, folder) ");
        qb.push_values(chunk, |mut b, (name, path, folder)| {
            b.push_bind(name).push_bind(path).push_bind(folder);
        });
        let result = qb
            .build()
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        inserted += result.rows_affected() as usize;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(inserted)
}

#[derive(Clone, Serialize)]
struct IndexProgress {
    indexed: usize,
    done: bool,
}

/// Varre `path` e indexa as imagens. Não devolve a lista: o frontend deve
/// escutar o evento "index-progress" e depois buscar as páginas com
/// `get_indexed_images`. Retorna o total de imagens encontradas.
#[tauri::command]
async fn index_images(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let db = state.db.clone();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<ItemBatch>>(8);

    // Produtor: só varre o disco (bloqueante), em lotes de 500.
    let walker = tokio::task::spawn_blocking(move || {
        let mut batch: Vec<ItemBatch> = Vec::with_capacity(500);

        for entry in walkdir::WalkDir::new(&path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let p = entry.path();
            if !is_image(p) {
                continue;
            }
            let (Some(name), Some(parent)) = (p.file_name(), p.parent()) else {
                continue;
            };

            batch.push((
                name.to_string_lossy().into_owned(),
                p.to_string_lossy().into_owned(),
                parent.to_string_lossy().into_owned(),
            ));

            if batch.len() >= 500 {
                // Se o consumidor parou (erro), encerra a varredura.
                if tx.blocking_send(std::mem::take(&mut batch)).is_err() {
                    return;
                }
            }
        }

        if !batch.is_empty() {
            let _ = tx.blocking_send(batch);
        }
    });

    // Consumidor: uma transação por lote + progresso para a UI.
    let mut total = 0usize;
    while let Some(batch) = rx.recv().await {
        save_batch_to_database(&batch, &db).await?;
        total += batch.len();
        let _ = app.emit(
            "index-progress",
            IndexProgress {
                indexed: total,
                done: false,
            },
        );
    }

    walker.await.map_err(|e| e.to_string())?;

    // Atualiza estatísticas do planejador de consultas após a carga.
    let _ = sqlx::query("PRAGMA optimize").execute(&db).await;

    let _ = app.emit(
        "index-progress",
        IndexProgress {
            indexed: total,
            done: true,
        },
    );
    Ok(total)
}

// ---------------------------------------------------------------------------
// Miniaturas e cores
// ---------------------------------------------------------------------------

/// Gera a miniatura (600px) e extrai as cores dominantes, se ainda não existirem.
/// Em caso de falha ao decodificar, devolve o caminho da imagem original para a
/// UI não ficar com uma imagem quebrada.
fn verify_or_generate_thumb(original_path: &str, cache_dir: &Path, db: &SqlitePool) -> String {
    let path_orig = Path::new(original_path);

    let full_thumb_path = cache_dir.join(format!("thumb_{}.jpg", short_hash(original_path)));
    let path_thumb_str = full_thumb_path.to_string_lossy().into_owned();
    let thumb_exists = full_thumb_path.exists();

    // Uma única consulta: id da imagem + se já tem cores.
    let (image_id, has_colors): (Option<i64>, bool) = tauri::async_runtime::block_on(async {
        match sqlx::query_as::<_, (i64, i64)>(
            "SELECT i.id, EXISTS(SELECT 1 FROM image_colors c WHERE c.image_id = i.id)
             FROM images i WHERE i.path = ?",
        )
        .bind(original_path)
        .fetch_optional(db)
        .await
        {
            Ok(Some((id, has))) => (Some(id), has != 0),
            _ => (None, false),
        }
    });

    let need_colors = image_id.is_some() && !has_colors;

    // Já tem arquivo e cores: nada a fazer.
    if thumb_exists && !need_colors {
        return path_thumb_str;
    }

    // Decodifica (with_guessed_format cobre extensões trocadas).
    let opened = image::io::Reader::open(path_orig)
        .and_then(|r| r.with_guessed_format())
        .ok()
        .and_then(|r| r.decode().ok());

    let Some(opened_image) = opened else {
        return if thumb_exists {
            path_thumb_str
        } else {
            original_path.to_string()
        };
    };

    let thumb = opened_image.thumbnail(600, 600);
    let mut thumb_ok = thumb_exists;

    if !thumb_exists {
        // Escreve em arquivo temporário e renomeia, para nunca deixar um JPEG
        // incompleto no cache se o app fechar no meio.
        let rgb_thumb = thumb.to_rgb8();
        // Nome único por chamada: dois lotes simultâneos processando a mesma imagem
        // não escrevem no mesmo arquivo (o rename final é atômico).
        let tmp_path = full_thumb_path.with_extension(format!(
            "{}.tmp",
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let written = std::fs::File::create(&tmp_path)
            .map(|f| {
                let mut w = BufWriter::new(f);
                JpegEncoder::new_with_quality(&mut w, 30)
                    .encode_image(&rgb_thumb)
                    .is_ok()
                    && w.flush().is_ok()
            })
            .unwrap_or(false);
        thumb_ok = written && std::fs::rename(&tmp_path, &full_thumb_path).is_ok();
        if !thumb_ok {
            let _ = std::fs::remove_file(&tmp_path);
        }
    }

    if need_colors {
        if let Some(id) = image_id {
            let mini = thumb
                .resize_exact(32, 32, image::imageops::FilterType::Triangle)
                .to_rgb8();

            let data: Vec<LinSrgb> = mini
                .pixels()
                .map(|p| {
                    Srgb::new(
                        p[0] as f32 / 255.0,
                        p[1] as f32 / 255.0,
                        p[2] as f32 / 255.0,
                    )
                    .into_linear()
                })
                .collect();

            let result = get_kmeans(4, 15, 0.005, false, &data, 0);

            // Peso de cada cor = fração de pixels atribuídos ao centroide.
            let total = result.indices.len().max(1) as f64;
            let mut counts = vec![0u32; result.centroids.len()];
            for &i in &result.indices {
                if let Some(c) = counts.get_mut(i as usize) {
                    *c += 1;
                }
            }

            // Todas as cores da imagem em uma única transação.
            tauri::async_runtime::block_on(async {
                if let Ok(mut tx) = db.begin().await {
                    for (idx, cent) in result.centroids.iter().enumerate() {
                        let srgb: Srgb<f32> = Srgb::from_linear(*cent);
                        let r = (srgb.red * 255.0).clamp(0.0_f32, 255.0_f32) as u8;
                        let g = (srgb.green * 255.0).clamp(0.0_f32, 255.0_f32) as u8;
                        let b = (srgb.blue * 255.0).clamp(0.0_f32, 255.0_f32) as u8;
                        let hex = format!("#{:02X}{:02X}{:02X}", r, g, b);
                        let weight = counts[idx] as f64 / total;

                        let _ = sqlx::query(
                            "INSERT INTO image_colors (image_id, hex_code, r_val, g_val, b_val, weight)
                             VALUES (?, ?, ?, ?, ?, ?)",
                        )
                        .bind(id)
                        .bind(hex)
                        .bind(r as i32)
                        .bind(g as i32)
                        .bind(b as i32)
                        .bind(weight)
                        .execute(&mut *tx)
                        .await;
                    }
                    let _ = tx.commit().await;
                }
            });
        }
    }

    if thumb_ok {
        path_thumb_str
    } else {
        original_path.to_string()
    }
}

#[tauri::command]
async fn generate_thumbnails_batch(
    caminhos: Vec<String>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<Vec<String>, String> {
    let cache_dir = app_handle
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?;
    let db = state.db.clone();

    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let results: Vec<String> = tokio::task::spawn_blocking(move || {
        caminhos
            .into_par_iter()
            .map(|original_path| verify_or_generate_thumb(&original_path, &cache_dir, &db))
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(results)
}

// ---------------------------------------------------------------------------
// Consultas de imagens e pastas
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_indexed_images(
    page: u32,
    limit: u32,
    folder: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ImageEntry>, String> {
    let db = &state.db;
    let offset = page * limit;

    let rows = match folder {
        // Pasta exata: usa idx_images_folder_name (já vem ordenado por nome).
        Some(folder_path) => {
            sqlx::query_as::<_, ImageEntry>(
                "SELECT id, name, path, notes FROM images
                 WHERE folder = ? ORDER BY name LIMIT ? OFFSET ?",
            )
            .bind(folder_path)
            .bind(limit)
            .bind(offset)
            .fetch_all(db)
            .await
        }
        None => {
            sqlx::query_as::<_, ImageEntry>(
                "SELECT id, name, path, notes FROM images ORDER BY id DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(db)
            .await
        }
    }
    .map_err(|e| e.to_string())?;

    Ok(rows)
}

/// Lista de pastas (mesmo formato de antes), agora direto do índice.
#[tauri::command]
async fn get_indexed_folders(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT folder FROM images WHERE folder <> '' ORDER BY folder",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(|(f,)| f).collect())
}

/// Versão paginada, com contagem de imagens por pasta.
#[tauri::command]
async fn get_indexed_folders_page(
    offset: i64,
    limit: i64,
    state: State<'_, AppState>,
) -> Result<Vec<FolderEntry>, String> {
    sqlx::query_as::<_, FolderEntry>(
        "SELECT folder, COUNT(*) AS count FROM images
         WHERE folder <> '' GROUP BY folder ORDER BY folder LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn verify_and_update_paths(state: State<'_, AppState>) -> Result<String, String> {
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, path FROM images")
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // Checagem de disco fora da thread async.
    let missing: Vec<i64> = tokio::task::spawn_blocking(move || {
        rows.into_iter()
            .filter(|(_, p)| {
                let path = Path::new(p);
                // Só considera "sumido" se a pasta ainda existe: evita apagar o
                // índice inteiro quando um HD externo está desmontado.
                !path.exists() && path.parent().map_or(false, |d| d.exists())
            })
            .map(|(id, _)| id)
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;

    for chunk in missing.chunks(500) {
        sqlx::query("DELETE FROM images WHERE id IN (SELECT value FROM json_each(?))")
            .bind(serde_json::to_string(chunk).map_err(|e| e.to_string())?)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(format!(
        "Verificação concluída. {} registros removidos.",
        missing.len()
    ))
}

#[tauri::command]
async fn reset_index(state: State<'_, AppState>) -> Result<(), String> {
    sqlx::query("DELETE FROM images")
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM sqlite_sequence WHERE name='images'")
        .execute(&state.db)
        .await
        .ok();

    Ok(())
}

#[tauri::command]
async fn remove_indexed_folder(
    folder: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Faixa em vez de LIKE: usa o índice da UNIQUE(path), não trata '_' e '%'
    // como curingas e só pega o que está DENTRO da pasta ('/' + 1 == '0' em ASCII).
    // Ex.: remover "/home/u/arte" NÃO apaga "/home/u/arte_antiga".
    let base = folder.trim_end_matches('/');

    sqlx::query("DELETE FROM images WHERE path >= ? AND path < ?")
        .bind(format!("{base}/"))
        .bind(format!("{base}0"))
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok("Indexação da pasta removida com sucesso.".to_string())
}

#[tauri::command]
fn open_original_image(path: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Falha ao abrir imagem: {e}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err("Abrir imagem não é suportado nesta plataforma".to_string())
    }
}

#[tauri::command]
async fn save_image_note(
    id: i64,
    new_note: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("UPDATE images SET notes = ? WHERE id = ?")
        .bind(new_note)
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[tauri::command]
async fn create_tag(name: String, state: State<'_, AppState>) -> Result<i64, String> {
    let res = sqlx::query("INSERT OR IGNORE INTO tags (name) VALUES (?)")
        .bind(&name)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // Se a tag já existia, buscamos o ID dela; se não, pegamos o último inserido.
    if res.rows_affected() == 0 {
        let row = sqlx::query_as::<_, Tag>("SELECT id, name FROM tags WHERE name = ?")
            .bind(&name)
            .fetch_one(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.id)
    } else {
        Ok(res.last_insert_rowid())
    }
}

#[tauri::command]
async fn link_tag_image(
    image_id: i64,
    tag_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("INSERT OR IGNORE INTO image_tags (image_id, tag_id) VALUES (?, ?)")
        .bind(image_id)
        .bind(tag_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn unlink_tag_image(
    image_id: i64,
    tag_id: i64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM image_tags WHERE image_id = ? AND tag_id = ?")
        .bind(image_id)
        .bind(tag_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn delete_empty_tag(tag_id: i64, state: State<'_, AppState>) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM tags
         WHERE id = ?
           AND NOT EXISTS (
               SELECT 1 FROM image_tags WHERE image_tags.tag_id = tags.id
           )",
    )
    .bind(tag_id)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn list_tags_from_image(
    image_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<Tag>, String> {
    let tags = sqlx::query_as::<_, Tag>(
        "SELECT t.id, t.name FROM tags t
         INNER JOIN image_tags it ON t.id = it.tag_id
         WHERE it.image_id = ?",
    )
    .bind(image_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(tags)
}

#[tauri::command]
async fn get_images_by_tag(
    tag_name: String,
    page: u32,
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<ImageEntry>, String> {
    let offset = page * limit;

    // ORDER BY garante paginação estável entre as páginas.
    let images = sqlx::query_as::<_, ImageEntry>(
        "SELECT i.id, i.name, i.path, i.notes FROM images i
         INNER JOIN image_tags it ON i.id = it.image_id
         INNER JOIN tags t ON t.id = it.tag_id
         WHERE t.name = ?
         ORDER BY i.id DESC
         LIMIT ? OFFSET ?",
    )
    .bind(&tag_name)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(images)
}

#[tauri::command]
async fn get_all_tags(state: State<'_, AppState>) -> Result<Vec<Tag>, String> {
    let tags = sqlx::query_as::<_, Tag>("SELECT id, name FROM tags ORDER BY name ASC")
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(tags)
}

#[tauri::command]
async fn get_images_by_multiple_tags(
    tags: Vec<String>,
    page: u32,
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<ImageEntry>, String> {
    let db = &state.db;
    let offset = page * limit;

    // Sem duplicatas: o HAVING compara com a quantidade de tags distintas.
    let mut tags = tags;
    tags.sort();
    tags.dedup();

    if tags.is_empty() {
        return Ok(Vec::new());
    }

    // Filtra as relações que batem com os nomes, agrupa por imagem e exige que a
    // imagem tenha TODAS as tags pesquisadas (HAVING COUNT = quantidade de tags).
    let tags_quantity = tags.len() as i64;
    let placeholders: Vec<&str> = std::iter::repeat("?").take(tags.len()).collect();
    let query_string = format!(
        "SELECT i.id, i.name, i.path, i.notes
         FROM images i
         JOIN image_tags it ON i.id = it.image_id
         JOIN tags t ON it.tag_id = t.id
         WHERE t.name IN ({})
         GROUP BY i.id
         HAVING COUNT(DISTINCT t.id) = ?
         ORDER BY i.id DESC
         LIMIT ? OFFSET ?",
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<Sqlite, ImageEntry>(&query_string);
    for tag in tags {
        query = query.bind(tag);
    }

    let rows = query
        .bind(tags_quantity)
        .bind(limit)
        .bind(offset)
        .fetch_all(db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[tauri::command]
async fn link_tags_in_bulk(
    image_id: i64,
    tag_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Um único comando; OR IGNORE evita erro se o vínculo já existir.
    sqlx::query(
        "INSERT OR IGNORE INTO image_tags (image_id, tag_id)
         SELECT ?, value FROM json_each(?)",
    )
    .bind(image_id)
    .bind(serde_json::to_string(&tag_ids).map_err(|e| e.to_string())?)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn link_tags_and_images_in_bulk(
    image_ids: Vec<i64>,
    tag_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Produto cartesiano imagens x tags em um único comando (atômico).
    sqlx::query(
        "INSERT OR IGNORE INTO image_tags (image_id, tag_id)
         SELECT i.value, t.value FROM json_each(?) AS i, json_each(?) AS t",
    )
    .bind(serde_json::to_string(&image_ids).map_err(|e| e.to_string())?)
    .bind(serde_json::to_string(&tag_ids).map_err(|e| e.to_string())?)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn remove_tags_and_images_in_bulk(
    image_ids: Vec<i64>,
    tag_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM image_tags
         WHERE image_id IN (SELECT value FROM json_each(?))
         AND tag_id IN (SELECT value FROM json_each(?))",
    )
    .bind(serde_json::to_string(&image_ids).map_err(|e| e.to_string())?)
    .bind(serde_json::to_string(&tag_ids).map_err(|e| e.to_string())?)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Imagens avulsas, caminhos e cores
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_or_create_image(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<i64, String> {
    // Upsert atômico: sem corrida entre SELECT e INSERT. O DO UPDATE é um no-op
    // que só existe para o RETURNING devolver o id da linha já existente.
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO images (path, name, folder) VALUES (?, ?, ?)
         ON CONFLICT(path) DO UPDATE SET path = excluded.path
         RETURNING id",
    )
    .bind(&path)
    .bind(&name)
    .bind(folder_of(&path))
    .fetch_one(&state.db)
    .await
    .map_err(|e| format!("Erro ao salvar imagem: {}", e))?;

    Ok(id)
}

#[tauri::command]
async fn search_image_path(state: State<'_, AppState>, id: i64) -> Result<String, String> {
    let path = sqlx::query_scalar::<_, String>("SELECT path FROM images WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| format!("Erro ao buscar imagem: {}", e))?
        .ok_or_else(|| "Imagem não encontrada".to_string())?;

    Ok(path)
}

#[tauri::command]
async fn search_images_by_color(
    r_target: i32,
    g_target: i32,
    b_target: i32,
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<ImageEntry>, String> {
    let db = &state.db;

    // Soma dos quadrados das diferenças (distância euclidiana sem a raiz).
    // O BETWEEN pré-filtra usando idx_image_colors_rgb.
    let rows = sqlx::query_as::<_, ImageEntry>(
        "SELECT i.id, i.name, i.path, i.notes
         FROM image_colors c
         JOIN images i ON c.image_id = i.id
         WHERE c.r_val BETWEEN ? - 45 AND ? + 45
           AND c.g_val BETWEEN ? - 45 AND ? + 45
           AND c.b_val BETWEEN ? - 45 AND ? + 45
         GROUP BY i.id
         ORDER BY MIN((c.r_val - ?) * (c.r_val - ?) +
                      (c.g_val - ?) * (c.g_val - ?) +
                      (c.b_val - ?) * (c.b_val - ?)) ASC
         LIMIT ?",
    )
    .bind(r_target)
    .bind(r_target)
    .bind(g_target)
    .bind(g_target)
    .bind(b_target)
    .bind(b_target)
    .bind(r_target)
    .bind(r_target)
    .bind(g_target)
    .bind(g_target)
    .bind(b_target)
    .bind(b_target)
    .bind(limit)
    .fetch_all(db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[tauri::command]
async fn get_image_main_color(
    image_id: i32,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Cor mais dominante = maior peso.
    let hex = sqlx::query_scalar::<_, String>(
        "SELECT hex_code FROM image_colors WHERE image_id = ? ORDER BY weight DESC, id LIMIT 1",
    )
    .bind(image_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    hex.ok_or_else(|| "Nenhuma cor indexada para esta imagem".to_string())
}

#[tauri::command]
async fn search_image_paths(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    id: i64,
) -> Result<MipmapPaths, String> {
    let original_path_str = sqlx::query_scalar::<_, String>("SELECT path FROM images WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| format!("Erro ao buscar imagem: {}", e))?
        .ok_or_else(|| "Imagem não encontrada".to_string())?;

    tokio::task::spawn_blocking(move || {
        let origin_path = Path::new(&original_path_str);
        if !origin_path.exists() {
            return Err("Arquivo original não encontrado no disco".to_string());
        }

        let cache_dir = app_handle
            .path()
            .app_cache_dir()
            .map_err(|_| "Não foi possível acessar a pasta de cache".to_string())?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

        // Nome baseado no hash do caminho completo (sem colisão entre pastas).
        let hash = short_hash(&original_path_str);
        let mid_path = cache_dir.join(format!("mid_{hash}.jpg"));
        let thumb_path = cache_dir.join(format!("small_{hash}.jpg"));

        // Lazy: só abre a imagem pesada se alguma das versões não existir.
        if !mid_path.exists() || !thumb_path.exists() {
            let img = image::open(origin_path).map_err(|e| e.to_string())?;

            if !mid_path.exists() {
                img.resize(1200, 1200, image::imageops::FilterType::Triangle)
                    .to_rgb8()
                    .save(&mid_path)
                    .map_err(|e| e.to_string())?;
            }

            if !thumb_path.exists() {
                img.resize(300, 300, image::imageops::FilterType::Triangle)
                    .to_rgb8()
                    .save(&thumb_path)
                    .map_err(|e| e.to_string())?;
            }
        }

        Ok(MipmapPaths {
            original: original_path_str.clone(),
            mid: mid_path.to_string_lossy().into_owned(),
            thumb: thumb_path.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Moodboards
// ---------------------------------------------------------------------------

#[tauri::command]
async fn save_moodboard(
    state: State<'_, AppState>,
    dto: SalvarMoodboardDTO,
) -> Result<i64, String> {
    let pool = &state.db;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Erro ao iniciar transação: {}", e))?;

    let moodboard_id = if let Some(id) = dto.id {
        sqlx::query(
            "UPDATE moodboards SET name = ?, x_camera = ?, y_camera = ?, zoom_camera = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(&dto.name)
        .bind(dto.x_camera)
        .bind(dto.y_camera)
        .bind(dto.zoom_camera)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao atualizar moodboard: {}", e))?;

        id
    } else {
        let result = sqlx::query(
            "INSERT INTO moodboards (name, x_camera, y_camera, zoom_camera) VALUES (?, ?, ?, ?)",
        )
        .bind(&dto.name)
        .bind(dto.x_camera)
        .bind(dto.y_camera)
        .bind(dto.zoom_camera)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao salvar moodboard: {}", e))?;

        result.last_insert_rowid()
    };

    // Substitui os itens antigos pelos novos.
    sqlx::query("DELETE FROM moodboard_items WHERE moodboard_id = ?")
        .bind(moodboard_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao deletar itens antigos: {}", e))?;

    for item in dto.items {
        let (unique_type, x, y, width, height, properties) = match item {
            MoodboardItem::Image {
                id: _,
                x,
                y,
                width,
                height,
                properties,
            } => {
                let json = serde_json::to_string(&properties).map_err(|e| e.to_string())?;
                ("image", x, y, width, height, json)
            }
            MoodboardItem::Drawing {
                id: _,
                x,
                y,
                width,
                height,
                properties,
            } => {
                let json = serde_json::to_string(&properties).map_err(|e| e.to_string())?;
                ("drawing", x, y, width, height, json)
            }
        };

        sqlx::query(
            "INSERT INTO moodboard_items (moodboard_id, unique_type, x, y, width, height, properties) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(moodboard_id)
        .bind(unique_type)
        .bind(x)
        .bind(y)
        .bind(width)
        .bind(height)
        .bind(properties)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao salvar item: {}", e))?;
    }

    tx.commit()
        .await
        .map_err(|e| format!("Erro ao commitar: {}", e))?;

    Ok(moodboard_id)
}

#[tauri::command]
async fn get_moodboard_with_items(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(Moodboard, Vec<MoodboardItem>), String> {
    let pool = &state.db;

    let moodboard = sqlx::query_as::<_, Moodboard>("SELECT * FROM moodboards WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Erro ao buscar moodboard: {}", e))?
        .ok_or_else(|| "Moodboard não encontrado".to_string())?;

    let items_raw = sqlx::query_as::<_, (i64, String, f64, f64, f64, f64, String)>(
        "SELECT id, unique_type, x, y, width, height, properties FROM moodboard_items WHERE moodboard_id = ?",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Erro ao buscar itens: {}", e))?;

    let mut items = Vec::new();
    for (item_id, unique_type, x, y, width, height, properties) in items_raw {
        match unique_type.as_str() {
            "image" => {
                let props: ItemImageProperties = serde_json::from_str(&properties)
                    .map_err(|e| format!("Erro ao parsear propriedades de imagem: {}", e))?;

                items.push(MoodboardItem::Image {
                    id: item_id.to_string(),
                    x,
                    y,
                    width,
                    height,
                    properties: props,
                });
            }
            "drawing" => {
                let props: ItemDrawingProperties = serde_json::from_str(&properties)
                    .map_err(|e| format!("Erro ao parsear propriedades de desenho: {}", e))?;

                items.push(MoodboardItem::Drawing {
                    id: item_id.to_string(),
                    x,
                    y,
                    width,
                    height,
                    properties: props,
                });
            }
            _ => return Err(format!("unique_type de item desconhecido: {}", unique_type)),
        }
    }

    Ok((moodboard, items))
}

#[tauri::command]
async fn list_moodboards(state: State<'_, AppState>) -> Result<Vec<Moodboard>, String> {
    sqlx::query_as::<_, Moodboard>("SELECT * FROM moodboards ORDER BY updated_at DESC")
        .fetch_all(&state.db)
        .await
        .map_err(|e| format!("Erro ao listar moodboards: {}", e))
}

#[tauri::command]
async fn delete_moodboard(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM moodboards WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("Erro ao deletar moodboard: {}", e))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Banco de dados: conexão, schema, migração e índices
// ---------------------------------------------------------------------------

async fn init_db(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .pragma("cache_size", "-20000");
    // Obs.: o cache de statements do sqlx foi deixado no padrão (LRU de 100).
    // Desativá-lo (statement_cache_capacity(0)) faz cada query ser recompilada.

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    // --- Tabelas -----------------------------------------------------------
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS images (
            id INTEGER PRIMARY KEY,
            name TEXT,
            path TEXT UNIQUE,
            folder TEXT NOT NULL DEFAULT '',
            notes TEXT DEFAULT ''
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS image_tags (
            image_id INTEGER,
            tag_id INTEGER,
            PRIMARY KEY (image_id, tag_id),
            FOREIGN KEY (image_id) REFERENCES images(id) ON DELETE CASCADE,
            FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS image_colors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            image_id INTEGER NOT NULL,
            hex_code TEXT NOT NULL,
            r_val INTEGER NOT NULL,
            g_val INTEGER NOT NULL,
            b_val INTEGER NOT NULL,
            weight REAL NOT NULL, -- Grau de dominância da cor (0.0 a 1.0)
            FOREIGN KEY (image_id) REFERENCES images(id) ON DELETE CASCADE
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS moodboards (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            x_camera REAL NOT NULL DEFAULT 0,
            y_camera REAL NOT NULL DEFAULT 0,
            zoom_camera REAL NOT NULL DEFAULT 1,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS moodboard_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            moodboard_id INTEGER NOT NULL,
            unique_type TEXT NOT NULL CHECK (unique_type IN ('image', 'drawing')),
            x REAL NOT NULL,
            y REAL NOT NULL,
            width REAL NOT NULL,
            height REAL NOT NULL,
            properties TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (moodboard_id) REFERENCES moodboards(id) ON DELETE CASCADE
        )",
    )
    .execute(&pool)
    .await?;

    // --- Migração: coluna `folder` em bancos criados antes dela --------------
    let has_folder: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info('images') WHERE name = 'folder'")
            .fetch_one(&pool)
            .await?;

    if has_folder == 0 {
        sqlx::query("ALTER TABLE images ADD COLUMN folder TEXT NOT NULL DEFAULT ''")
            .execute(&pool)
            .await?;

        // dirname em SQL puro (caminhos com '/'): remove o nome do arquivo.
        sqlx::query(
            "UPDATE images SET folder = substr(
                 rtrim(path, replace(path, '/', '')), 1,
                 length(rtrim(path, replace(path, '/', ''))) - 1)",
        )
        .execute(&pool)
        .await?;
    }

    // --- Índices (depois da migração, pois dependem da coluna folder) --------
    let indexes = [
        // Listagem de imagens por pasta, já ordenada por nome.
        "CREATE INDEX IF NOT EXISTS idx_images_folder_name ON images(folder, name)",
        // FK + cascade + checagem de cores: sem isso viram scans completos.
        "CREATE INDEX IF NOT EXISTS idx_image_colors_image ON image_colors(image_id)",
        "CREATE INDEX IF NOT EXISTS idx_image_colors_rgb ON image_colors(r_val, g_val, b_val)",
        // Buscas por tag e cascade de tags.
        "CREATE INDEX IF NOT EXISTS idx_image_tags_tag ON image_tags(tag_id)",
        "CREATE INDEX IF NOT EXISTS idx_moodboard_items_moodboard ON moodboard_items(moodboard_id)",
        "CREATE INDEX IF NOT EXISTS idx_moodboard_items_unique_type ON moodboard_items(unique_type)",
    ];
    for sql in indexes {
        sqlx::query(sql).execute(&pool).await?;
    }

    let _ = sqlx::query("PRAGMA optimize").execute(&pool).await;

    Ok(pool)
}

// ---------------------------------------------------------------------------
// Entrada do app
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            // Pool global do rayon (decodificação de imagens): 3 threads.
            rayon::ThreadPoolBuilder::new()
                .num_threads(12)
                .build_global()
                .ok();

            let app_data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            std::fs::create_dir_all(&cache_dir)?;

            let database_name = if cfg!(debug_assertions) {
                "propositum_development.db"
            } else {
                "propositum_production.db"
            };
            let db_path = app_data_dir.join(database_name);
            println!("Conectando ao banco em: {}", db_path.display());

            let pool = tauri::async_runtime::block_on(init_db(&db_path))?;
            app.manage(AppState { db: pool });

            let scope = app.asset_protocol_scope();
            // A pasta de cache fica em ~/.cache/..., um diretório oculto (começa com ponto).
            // O padrão "/**" NÃO casa com diretórios ocultos no Linux, então a pasta de
            // miniaturas precisa ser liberada explicitamente. Não remova esta linha.
            scope.allow_directory(&cache_dir, true).ok();
            // Atenção: libera o protocolo asset para o sistema de arquivos inteiro
            // (necessário para exibir as imagens originais de qualquer pasta).
            scope.allow_directory("/", true).ok();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            index_images,
            get_indexed_images,
            reset_index,
            generate_thumbnails_batch,
            open_original_image,
            get_indexed_folders,
            get_indexed_folders_page,
            verify_and_update_paths,
            remove_indexed_folder,
            create_tag,
            link_tag_image,
            unlink_tag_image,
            list_tags_from_image,
            get_images_by_tag,
            get_all_tags,
            delete_empty_tag,
            get_images_by_multiple_tags,
            link_tags_in_bulk,
            link_tags_and_images_in_bulk,
            remove_tags_and_images_in_bulk,
            save_image_note,
            get_or_create_image,
            search_image_path,
            search_images_by_color,
            get_image_main_color,
            save_moodboard,
            get_moodboard_with_items,
            list_moodboards,
            delete_moodboard,
            search_image_paths,
            close_splashscreen
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}