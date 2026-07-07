use tauri::{Manager, State, Emitter};
use tokio::task;
use std::process::Command;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::path::Path;
use std::collections::HashSet;
use sqlx::{Sqlite, SqlitePool, Row};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use walkdir::WalkDir;
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::path::{PathBuf};

use tauri::{AppHandle};

pub struct AppState {
    pub db: SqlitePool,
}   

#[derive(serde::Serialize, sqlx::FromRow)]
pub struct ImageEntry {
    pub id: i32,
    pub name: String,
    pub path: String,
    pub notes: String
}

#[derive(Debug, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
pub struct MipmapPaths {
    original: String,
    mid: String,
    thumb: String,
}

use serde_json;

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

#[tauri::command]
async fn close_splashscreen(app_handle: AppHandle) {
    // 1. Localiza a janela principal (main)
    if let Some(main_window) = app_handle.get_webview_window("main") {
        // Exibe a janela principal
        let _ = main_window.show();
        // Dá o foco para ela ir para a frente
        let _ = main_window.set_focus();
    } else {
        println!("ERRO: Janela 'main' não foi encontrada!");
    }

    // 2. Localiza a janela da splashscreen e a fecha
    if let Some(splash_window) = app_handle.get_webview_window("splashscreen") {
        let _ = splash_window.close();
    } else {
        println!("Aviso: Janela 'splashscreen' não foi encontrada ou já estava fechada.");
    }
}

// Padrão original de 2 elementos para inserção em lote
type ItemBatch = (String, String);
async fn save_batch_to_database(lote: &Vec<ItemBatch>, db: &sqlx::SqlitePool) -> Result<usize, String> {
    if lote.is_empty() { return Ok(0); }

    let mut tx = db.begin().await.map_err(|e| e.to_string())?;

    // Inserção simples e direta apenas em nome e caminho
    let mut query_builder = sqlx::QueryBuilder::new(
        "INSERT OR IGNORE INTO images (name, path) "
    );
    
    query_builder.push_values(lote, |mut b, item| {
        b.push_bind(&item.0)   // nome
         .push_bind(&item.1);  // caminho
    });

    let query = query_builder.build();
    let result = query.execute(&mut *tx).await.map_err(|e| e.to_string())?;
    let inserted = result.rows_affected() as usize;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(inserted)
}

#[tauri::command]
async fn index_images(
    path: String, 
    state: tauri::State<'_, AppState>
) -> Result<Vec<ImageEntry>, String> {
    let db = state.db.clone();
    
    // Executa a varredura do SSD de forma ultra-rápida na thread pool do Tokio
    let images_list = tokio::task::spawn_blocking(move || {
        let mut batch: Vec<(String, String)> = Vec::new();
        let mut generated_images: Vec<ImageEntry> = Vec::new();
        let mut id_virtual = 1;

        let iterator = walkdir::WalkDir::new(&path).into_iter().filter_map(|e| e.ok());

        for entry in iterator {
            if entry.file_type().is_file() {
                let p = entry.path();
                if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                    if matches!(ext.to_lowercase().as_str(), "jpg" | "png" | "jpeg" | "webp") {
                        
                        let name = p.file_name().unwrap().to_string_lossy().to_string();
                        let original_path = p.to_string_lossy().to_string();
                        
                        // Alimenta a lista em memória APENAS com o que já existe
                        generated_images.push(ImageEntry {
                            id: id_virtual,
                            name: name.clone(),
                            path: original_path.clone(),
                            notes: String::new(),
                        });

                        id_virtual += 1;
                        batch.push((name, original_path));

                        // Salva no banco de dados de 100 em 100 para alta performance
                        if batch.len() >= 100 {
                            let currently_batch = std::mem::take(&mut batch);
                            let db_clone = db.clone();
                            tauri::async_runtime::handle().block_on(async move {
                                let _ = save_batch_to_database(&currently_batch, &db_clone).await;
                            });
                        }
                    }
                }
            }
        }

        // Salva o restante dos itens no banco
        if !batch.is_empty() {
            tauri::async_runtime::handle().block_on(async move {
                let _ = save_batch_to_database(&batch, &db).await;
            });
        }

        generated_images
    }).await.map_err(|e| e.to_string())?;

    Ok(images_list)
}

fn verify_or_generate_thumb(original_path: &str, cache_dir: &std::path::Path, db: &sqlx::SqlitePool) -> String {
    let path_orig = std::path::Path::new(original_path);
    
    let arquive_name = path_orig.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown_image.jpg".to_string());

    let thumb_name = format!("thumb_{}.jpg", arquive_name.replace(" ", "_"));
    let full_thumb_path = cache_dir.join(&thumb_name);
    let path_thumb_str = full_thumb_path.to_string_lossy().to_string();

    let thumb_exists = full_thumb_path.exists();
    
    let mut need_extract_colors = true;
    let mut image_id: Option<i32> = None;

    // 1. Verifica no banco se a imagem já foi indexada e se já possui cores
    tauri::async_runtime::handle().block_on(async {
        if let Ok(Some(row)) = sqlx::query("SELECT id FROM images WHERE path = ?")
            .bind(original_path)
            .fetch_optional(db)
            .await 
        {
            let id: i32 = row.get("id");
            image_id = Some(id);

            // Se já existir pelo menos uma cor para essa imagem, não precisamos reprocessar o K-Means
            if let Ok(Some(_)) = sqlx::query("SELECT 1 FROM image_colors WHERE image_id = ? LIMIT 1")
                .bind(id)
                .fetch_optional(db)
                .await
            {
                need_extract_colors = false;
            }
        }
    });

    // 🚀 OTIMIZAÇÃO: Se o arquivo já existe E as cores já estão no banco, mata o processamento aqui
    if thumb_exists && !need_extract_colors {
        return path_thumb_str;
    }

    // 2. Se caiu aqui, ou falta o arquivo de thumb, ou faltam as cores (ou ambos)
    if let Ok(reader) = image::io::Reader::open(path_orig) {
        if let Ok(opened_image) = reader.decode() {
            
            let thumb = opened_image.thumbnail(600, 600);
            
            // Só grava o arquivo no SSD se ele realmente não existir
            if !thumb_exists {
                if let Ok(mut f) = std::fs::File::create(&full_thumb_path) {
                    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, 30);
                    let _ = encoder.encode_image(&thumb);
                }
            }

            // Só roda o algoritmo pesado de K-Means se o banco de dados estiver sem as cores dessa imagem
            if need_extract_colors {
                if let Some(id) = image_id {
                    let mini_img = thumb.resize_exact(32, 32, image::imageops::FilterType::Triangle);
                    let pixels = mini_img.to_rgb8();
                    
                    use kmeans_colors::get_kmeans;
                    use palette::{Srgb, LinSrgb};

                    let data: Vec<LinSrgb> = pixels.pixels()
                        .map(|p| Srgb::new(
                            p[0] as f32 / 255.0, 
                            p[1] as f32 / 255.0, 
                            p[2] as f32 / 255.0
                        ).into_linear())
                        .collect();

                    let result = get_kmeans(4, 15, 0.005, false, &data, 0);

                    tauri::async_runtime::handle().block_on(async {
                        for cent in result.centroids {
                            let srgb_cor: Srgb<f32> = Srgb::from_linear(cent);
                            let r = (srgb_cor.red * 255.0).clamp(0.0_f32, 255.0_f32) as u8;
                            let g = (srgb_cor.green * 255.0).clamp(0.0_f32, 255.0_f32) as u8;
                            let b = (srgb_cor.blue * 255.0).clamp(0.0_f32, 255.0_f32) as u8;
                            
                            let hex = format!("#{:02X}{:02X}{:02X}", r, g, b);

                            let _ = sqlx::query(
                                "INSERT INTO image_colors (image_id, hex_code, r_val, g_val, b_val, weight) VALUES (?, ?, ?, ?, ?, 1.0)"
                            )
                            .bind(id)
                            .bind(hex)
                            .bind(r as i32)
                            .bind(g as i32)
                            .bind(b as i32)
                            .execute(db)
                            .await;
                        }
                    });
                }
            }
        }
    }

    path_thumb_str
}

#[tauri::command]
async fn generate_thumbnails_batch(
    caminhos: Vec<String>, 
    state: tauri::State<'_, AppState>, // Adicionado para acessar o DB
    app_handle: tauri::AppHandle
) -> Result<Vec<String>, String> {
    let cache_dir = app_handle.path().app_cache_dir().map_err(|e| e.to_string())?;
    let db = state.db.clone(); // Clone do pool para levar à thread pool

    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    
    use rayon::prelude::*;
    
    let results: Vec<String> = tokio::task::spawn_blocking(move || {
        caminhos.into_par_iter().map(|original_path| {
            // Repassa o db_clone aqui
            verify_or_generate_thumb(&original_path, &cache_dir, &db)
        }).collect()
    }).await.map_err(|e| e.to_string())?;

    Ok(results)
}

#[tauri::command]
async fn get_indexed_images(
    page: u32,
    limit: u32,
    folder: Option<String>, // Agora o Rust vai usar esse cara!
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<ImageEntry>, String> {
    let db = &state.db;
    let offset = page * limit;
    let cache_dir = app_handle.path().app_cache_dir().map_err(|e| e.to_string())?;

    // Avalia se o frontend passou uma pasta ou se quer ver tudo
    let rows = match folder {
        Some(folder_path) => {
            // Se veio uma pasta, buscamos imagens cujo caminho comece com o caminho dela
            // Adicionamos o '%' no final para o SQL buscar subpastas/arquivos contidos nela
            let folder_filter = format!("{}%", folder_path);

            sqlx::query_as::<_, ImageEntry>(
                "SELECT id, name, path, notes FROM images WHERE path LIKE ? ORDER BY id DESC LIMIT ? OFFSET ?"
            )
            .bind(folder_filter)
            .bind(limit)
            .bind(offset)
            .fetch_all(db)
            .await
        }
        None => {
            // Se for None (Todas as Imagens), roda a query padrão original
            sqlx::query_as::<_, ImageEntry>(
                "SELECT id, name, path, notes FROM images ORDER BY id DESC LIMIT ? OFFSET ?"
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(db)
            .await
        }
    }.map_err(|e| e.to_string())?;

    Ok(rows)
}

#[tauri::command]
async fn verify_and_update_paths(state: tauri::State<'_, AppState>) -> Result<String, String> {
    
    // 1. Buscamos todas as imagens usando o query_as estruturado para evitar erros de inferência
    let rows = sqlx::query_as::<_, ImageEntry>("SELECT id, name, path, notes FROM images")
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    let updated = 0;
    let mut deleted = 0;

    // Se você quiser passar a lista de pastas do frontend para fazer o scan inteligente depois,
    // podemos receber como parâmetro. Por enquanto, a checagem de existência e limpeza:
    for row in rows {
        let currently_path = Path::new(&row.path);

        // Se o arquivo NÃO existe mais no HD/SSD, precisamos agir
        if !currently_path.exists() {
            let found_new_path = false;

            // [Aqui entrará a sua lógica de adivinhação por pastas se quiser implementar depois]

            if !found_new_path {
                // Se sumiu e não achamos em lugar nenhum, deletamos usando a query dinâmica padrão
                sqlx::query("DELETE FROM images WHERE id = ?")
                    .bind(row.id)
                    .execute(&state.db)
                    .await
                    .map_err(|e| e.to_string())?;
                
                deleted += 1;
            }
        }
    }

    Ok(format!(
        "Verificação concluída. {} caminhos updated, {} registros limpos.", 
        updated, deleted
    ))
}

#[tauri::command]
async fn reset_index(state: State<'_, AppState>)-> Result<(), String>{
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
async fn remove_indexed_folder(folder: String, state: tauri::State<'_, AppState>) -> Result<String, String> {
    // Criamos o termo de busca adicionando o caractere curinga '%' no final.
    // Exemplo: "/home/user/pasta/%" garante que só vai deletar o que estiver DENTRO dela.
    let search_term = format!("{}%", folder);

    sqlx::query("DELETE FROM images WHERE path LIKE ?")
        .bind(search_term)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("Indexação da pasta removida com sucesso."))
}

#[tauri::command]
fn open_original_image(path: String) {
    // No Linux (Ubuntu), usamos o 'xdg-open'
    // No Windows, usaríamos 'explorer'
    #[cfg(target_os = "linux")]
    Command::new("xdg-open")
        .arg(&path)
        .spawn()
        .expect("Falha ao abrir imagem");
}

#[tauri::command]
async fn get_indexed_folders(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    // 1. Buscamos TODOS os caminhos de imagem direto do banco
    let rows: Vec<(String,)> = sqlx::query_as("SELECT path FROM images")
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // 2. Usamos um HashSet para garantir que as pastas não se repitam
    let mut unique_folders = HashSet::new();

    for (path,) in rows {
        if let Some(parent) = Path::new(&path).parent() {
            if let Some(folder_str) = parent.to_str() {
                if !folder_str.is_empty() {
                    unique_folders.insert(folder_str.to_string());
                }
            }
        }
    }

    // 3. Convertemos de volta para um vetor ordenado
    let mut result: Vec<String> = unique_folders.into_iter().collect();
    result.sort();

    Ok(result)
}

#[tauri::command]
async fn save_image_note(
    id: i64, 
    new_note: String, 
    state: tauri::State<'_, AppState> // 🚀 Puxa o AppState em vez do pool direto
) -> Result<(), String> {
    // Extrai o pool de dentro do seu AppState
    let pool = &state.db;

    sqlx::query("UPDATE images SET notes = ? WHERE id = ?")
        .bind(new_note)
        .bind(id)
        .execute(pool) // Usa o pool extraído
        .await
        .map_err(|e| e.to_string())?;
        
    Ok(())
}

// comandos de tags abaixo........................................................
#[tauri::command]
async fn create_tag(name: String, state: State<'_, AppState>) -> Result<i64, String> {
    let res = sqlx::query("INSERT OR IGNORE INTO tags (name) VALUES (?)")
        .bind(&name)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // Se a tag já existia, buscamos o ID dela, se não, pegamos o último inserido
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
async fn link_tag_image(image_id: i64, tag_id: i64, state: State<'_, AppState>) -> Result<(), String> {
    sqlx::query("INSERT OR IGNORE INTO image_tags (image_id, tag_id) VALUES (?, ?)")
        .bind(image_id)
        .bind(tag_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn unlink_tag_image(image_id: i64, tag_id: i64, state: State<'_, AppState>) -> Result<(), String> {
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
           )"
    )
    .bind(tag_id)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn list_tags_from_image(image_id: i64, state: State<'_, AppState>) -> Result<Vec<Tag>, String> {
    let tags = sqlx::query_as::<_, Tag>(
        "SELECT t.id, t.name FROM tags t 
         INNER JOIN image_tags it ON t.id = it.tag_id 
         WHERE it.image_id = ?"
    )
    .bind(image_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(tags)
}

#[tauri::command]
async fn get_images_by_tag(tag_name: String, page: u32, limit: u32, state: tauri::State<'_, AppState>,) ->Result<Vec<ImageEntry>, String> {
    let offset =  page * limit; 

    // Esta query cruza as três tabelas para trazer apenas as imagens que possuem a tag selecionada
    let images = sqlx::query_as::<_, ImageEntry>(
        "SELECT i.id, i.name, i.path, i.notes FROM images i
         INNER JOIN image_tags it ON i.id = it.image_id
         INNER JOIN tags t ON t.id = it.tag_id
         WHERE t.name = ?
         LIMIT ? OFFSET ?"
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
async fn get_all_tags(state: tauri::State<'_, AppState>) -> Result<Vec<Tag>, String> {
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
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<Vec<ImageEntry>, String> {
    let db = &state.db;
    let offset = page * limit;
    let cache_dir = app_handle.path().app_cache_dir().map_err(|e| e.to_string())?;

    if tags.is_empty() {
        return Ok(Vec::new());
    }

    // O truque da Query: 
    // 1. Filtramos apenas as relações que batem com os nomes passados na lista.
    // 2. Agrupamos por imagem_id.
    // 3. O HAVING COUNT garante que a imagem possua EXATAMENTE a quantidade de tags pesquisadas.
    // Ex: Se pesquisei por ["sketch", "character"], o count deve ser igual a 2.
    let tags_quantity = tags.len() as i64;

    // Criamos placeholders dinâmicos (?, ?, ?) com base no número de tags enviadas
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

    let mut query = sqlx::query_as::<sqlx::Sqlite, ImageEntry>(&query_string);

    // Vincula cada nome de tag dinamicamente
    for tag in tags {
        query = query.bind(tag);
    }

    // Vincula os parâmetros finais: a contagem de tags, o limite e o offset da paginação
    let rows = query
        .bind(tags_quantity)
        .bind(limit)
        .bind(offset)
        .fetch_all(db)
        .await
        .map_err(|e| e.to_string())?;

    // // Preenche as miniaturas na RAM para o React
    // for img in &mut rows {
    //     let nome_thumb = format!("thumb_{}.jpg", img.nome.replace(" ", "_"));
    //     img.caminho_thumb = cache_dir.join(nome_thumb).to_string_lossy().to_string();
    // }

    Ok(rows)
}

#[tauri::command]
async fn link_tags_in_bulk(
    image_id: i64,
    tag_ids: Vec<i64>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut tx = state.db.begin().await.map_err(|e| e.to_string())?;

    for tag_id in tag_ids {
        // OR IGNORE evita que o app quebre se o usuário tentar re-adicionar algo que já existe
        sqlx::query("INSERT OR IGNORE INTO image_tags (image_id, tag_id) VALUES (?, ?)")
            .bind(image_id)
            .bind(tag_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn link_tags_and_images_in_bulk(
    image_ids: Vec<i64>,
    tag_ids: Vec<i64>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // Abre uma transação única para garantir performance bruta e segurança
    let mut tx = state.db.begin().await.map_err(|e| e.to_string())?;

    for id_image in &image_ids {
        for id_tag in &tag_ids {
            // O INSERT OR IGNORE impede erros caso alguma imagem já possua alguma dessas tags
            sqlx::query("INSERT OR IGNORE INTO image_tags (image_id, tag_id) VALUES (?, ?)")
                .bind(id_image)
                .bind(id_tag)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn remove_tags_and_images_in_bulk(
    image_ids: Vec<i64>,
    tag_ids: Vec<i64>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // Cria uma query dinâmica para remover os vínculos
    // Exemplo usando SQLx (ajuste para bater com os nomes da sua tabela intermediária, ex: imagem_tags)
    sqlx::query(
        "DELETE FROM image_tags 
         WHERE image_id IN (SELECT value FROM json_each(?)) 
         AND tag_id IN (SELECT value FROM json_each(?))"
    )
    .bind(serde_json::to_string(&image_ids).unwrap())
    .bind(serde_json::to_string(&tag_ids).unwrap())
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_or_create_image(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<i64, String> {
    let pool = &state.db;
    
    // 1. Verifica se já existe pelo path
    let exists = sqlx::query_as::<_, (i64,)>("SELECT id FROM images WHERE path = ?")
        .bind(&path)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Erro ao buscar imagem: {}", e))?;
    
    if let Some((id,)) = exists {
        println!("✅ Imagem já existe no banco com ID: {}", id);
        return Ok(id);
    }
    
    // 2. Se não existe, insere novo registro
    let mut tx = pool.begin().await.map_err(|e| format!("Erro ao iniciar transação: {}", e))?;
    
    let result = sqlx::query("INSERT INTO images (path, name) VALUES (?, ?)")
        .bind(&path)
        .bind(&name)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao salvar imagem: {}", e))?;
    
    tx.commit().await.map_err(|e| format!("Erro ao commitar: {}", e))?;
    
    let new_id = result.last_insert_rowid();
    println!("🆕 Nova imagem criada com ID: {}", new_id);
    
    Ok(new_id)
}

#[tauri::command]
async fn search_image_path(state: State<'_, AppState>, id: i64) -> Result<String, String> {
    let pool = &state.db;
    
    let path = sqlx::query_scalar::<_, String>("SELECT path FROM images WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
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
    state: tauri::State<'_, AppState>
) -> Result<Vec<ImageEntry>, String> {
    let db = &state.db;

    // A query calcula a soma dos quadrados das diferenças (distância euclidiana sem a raiz quadrada)
    // O índice idx_imagem_cores_rgb ajuda o banco a filtrar dados rapidamente
    let rows = sqlx::query_as::<_, ImageEntry>(
        // "SELECT i.id, i.name, i.path, i.notes 
        //  FROM image_colors c
        //  JOIN images i ON c.image_id = i.id
        //  WHERE c.r_val BETWEEN ? - 45 AND ? + 45
        //    AND c.g_val BETWEEN ? - 45 AND ? + 45
        //    AND c.b_val BETWEEN ? - 45 AND ? + 45
        //  ORDER BY ((c.r_val - ?) * (c.r_val - ?) + 
        //            (c.g_val - ?) * (c.g_val - ?) + 
        //            (c.b_val - ?) * (c.b_val - ?)) ASC
        //  LIMIT ?"
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
         LIMIT ?"
    )
    .bind(r_target).bind(r_target)
    .bind(g_target).bind(g_target)
    .bind(b_target).bind(b_target)
    .bind(r_target).bind(r_target)
    .bind(g_target).bind(g_target)
    .bind(b_target).bind(b_target)
    .bind(limit)
    .fetch_all(db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[tauri::command]
async fn get_image_main_color(image_id: i32, state: tauri::State<'_, AppState>) -> Result<String, String> {
    // Pega a primeira cor (geralmente a mais marcante se ordenada por peso ou ID)
    let row = sqlx::query("SELECT hex_code FROM image_colors WHERE image_id = ? LIMIT 1")
        .bind(image_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    match row {
        Some(r) => Ok(r.get("hex_code")),
        None => Err("Nenhuma cor indexada para esta imagem".to_string())
    }
}

#[tauri::command]
async fn save_moodboard(
    state: State<'_, AppState>,
    dto: SalvarMoodboardDTO,
) -> Result<i64, String> {
    let pool = &state.db;
    let mut tx = pool.begin().await.map_err(|e| format!("Erro ao iniciar transação: {}", e))?;
    
    let moodboard_id = if let Some(id) = dto.id {
        // UPDATE moodboard existente
        sqlx::query(
            "UPDATE moodboards SET name = ?, x_camera = ?, y_camera = ?, zoom_camera = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?"
        )
        .bind(&dto.name)
        .bind(&dto.x_camera)
        .bind(&dto.y_camera)
        .bind(&dto.zoom_camera)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao atualizar moodboard: {}", e))?;
        
        id
    } else {
        // INSERT novo moodboard
        let result = sqlx::query(
            "INSERT INTO moodboards (name, x_camera, y_camera, zoom_camera) VALUES (?, ?, ?, ?)"
        )
        .bind(&dto.name)
        .bind(&dto.x_camera)
        .bind(&dto.y_camera)
        .bind(&dto.zoom_camera)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao salvar moodboard: {}", e))?;
        
        result.last_insert_rowid()
    };
    
    // ❌ Deleta itens antigos - OU atualiza se quiser manter
    sqlx::query("DELETE FROM moodboard_items WHERE moodboard_id = ?")
        .bind(&moodboard_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao deletar itens antigos: {}", e))?;
    
    // ✅ Salva novos itens
    for item in dto.items {
        let (unique_type, x, y, width, height, properties) = match item {
            MoodboardItem::Image { id: _, x, y, width, height, properties } => {
                let props = ItemImageProperties {
                    id_image_catalog: properties.id_image_catalog,
                };
                let json = serde_json::to_string(&props).map_err(|e| e.to_string())?;
                ("image".to_string(), x, y, width, height, json)
            },
            MoodboardItem::Drawing { id: _, x, y, width, height, properties } => {
                let json = serde_json::to_string(&properties).map_err(|e| e.to_string())?;
                ("drawing".to_string(), x, y, width, height, json)
            },
        };
        
        sqlx::query(
            "INSERT INTO moodboard_items (moodboard_id, unique_type, x, y, width, height, properties) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&moodboard_id)
        .bind(&unique_type)
        .bind(&x)
        .bind(&y)
        .bind(&width)
        .bind(&height)
        .bind(&properties)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Erro ao salvar item: {}", e))?;
    }
    
    tx.commit().await.map_err(|e| format!("Erro ao commitar: {}", e))?;
    
    Ok(moodboard_id)
}

#[tauri::command]
async fn get_moodboard_with_items(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(Moodboard, Vec<MoodboardItem>), String> {
    let pool = &state.db;
    
    // 1. Obter moodboard
    let moodboard = sqlx::query_as::<_, Moodboard>(
        "SELECT * FROM moodboards WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("Erro ao buscar moodboard: {}", e))?
    .ok_or_else(|| "Moodboard não encontrado".to_string())?;
    
    // 2. Obter itens
    let items_raw = sqlx::query_as::<_, (i64, String, f64, f64, f64, f64, String)>(
        "SELECT id, unique_type, x, y, width, height, properties FROM moodboard_items WHERE moodboard_id = ?"
    )
    .bind(&id)
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
                    x, y, width, height,
                    properties: props,
                });
            },
            "drawing" => {
                let props: ItemDrawingProperties = serde_json::from_str(&properties)
                    .map_err(|e| format!("Erro ao parsear propriedades de desenho: {}", e))?;
                
                items.push(MoodboardItem::Drawing {
                    id: item_id.to_string(),
                    x, y, width, height,
                    properties: props,
                });
            },
            _ => return Err(format!("unique_type de item desconhecido: {}", unique_type)),
        }
    }
    
    Ok((moodboard, items))
}

#[tauri::command]
async fn list_moodboards(
    state: State<'_, AppState>,
) -> Result<Vec<Moodboard>, String> {
    let pool = &state.db;
    
    sqlx::query_as::<_, Moodboard>(
        "SELECT * FROM moodboards ORDER BY updated_at DESC"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Erro ao listar moodboards: {}", e))
}

#[tauri::command]
async fn delete_moodboard(
    state: State<'_, AppState>,
    id: i64,
) -> Result<(), String> {
    let pool = &state.db;
    
    sqlx::query("DELETE FROM moodboards WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| format!("Erro ao deletar moodboard: {}", e))?;
    
    Ok(())
}

#[tauri::command]
async fn search_image_paths(state: tauri::State<'_, AppState>, app_handle: tauri::AppHandle, id: i64) -> Result<MipmapPaths, String> {
    let pool = &state.db;
    
    // 1. Busca direta no banco sem precisar chamar a função antiga
    let original_path_str = sqlx::query_scalar::<_, String>("SELECT path FROM images WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Erro ao buscar imagem: {}", e))?
        .ok_or_else(|| "Imagem não encontrada".to_string())?;

    tokio::task::spawn_blocking(move || {
        let origin_path = Path::new(&original_path_str);
        if !origin_path.exists() {
            return Err("Arquivo original não encontrado no disco".to_string());
        }

        // 2. Define a pasta de cache temporário do Tauri para o seu App
        let cache_dir = app_handle.path().app_cache_dir()
            .map_err(|_| "Não foi possível acessar a pasta de cache".to_string())?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

        let file_stem = origin_path.file_stem().unwrap().to_str().unwrap();
        let mid_path = cache_dir.join(format!("{}_mid.jpg", file_stem));
        let thumb_path = cache_dir.join(format!("{}_thumb.jpg", file_stem));

        // 3. Processamento Lazy: Só abre a imagem pesada se uma das miniaturas não existir
        if !mid_path.exists() || !thumb_path.exists() {
            let img = image::open(origin_path).map_err(|e| e.to_string())?;

            if !mid_path.exists() {
                // Resize mantendo o aspect ratio para no máximo 1200px
                let mid_img = img.resize(1200, 1200, image::imageops::FilterType::Triangle);
                mid_img.save(&mid_path).map_err(|e| e.to_string())?;
            }

            if !thumb_path.exists() {
                // Resize para miniatura de no máximo 300px
                let thumb_img = img.resize(300, 300, image::imageops::FilterType::Triangle);
                thumb_img.save(&thumb_path).map_err(|e| e.to_string())?;
            }
        }

        Ok(MipmapPaths {
            original: original_path_str,
            mid: mid_path.to_string_lossy().into_owned(),
            thumb: thumb_path.to_string_lossy().into_owned(),
        })
    }).await.map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(3) 
                .build_global()
                .unwrap();

            let cache_dir = app.path().app_cache_dir().expect("Falha ao obter pasta de cache");
            let scope = app.asset_protocol_scope();
            scope.allow_directory(&cache_dir, true).ok();
            let app_data_dir = app.path().app_data_dir()
                .expect("Não foi possível acessar a pasta de dados do sistema");

            println!("DEBUG: Pasta base: {:?}", app_data_dir);

            std::fs::create_dir_all(&app_data_dir).expect("Erro ao criar pastas de dados");

            if let Err(e) = std::fs::create_dir_all(&app_data_dir) {
                panic!("Erro fatal: Não foi possível criar a pasta {:?}: {}", app_data_dir, e);
            }

            let database_name = if cfg!(debug_assertions) {
                "propositum_development.db"
            } else {
                "propositum_production.db"
            };

            let db_path = app_data_dir.join(database_name);
            let connection_str = format!("sqlite://{}", db_path.display());
            
            println!("Tentando conectar ao banco em: {}", connection_str);

            tauri::async_runtime::block_on(async {
                let database_name_async = if cfg!(debug_assertions) { "propositum_development.db" } else { "propositum_production.db" };
                let db_path = app_data_dir.join(database_name_async); 
                let options = sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(&db_path)
                    .create_if_missing(true)
                    .statement_cache_capacity(0) // PARALISA O VAZAMENTO DE RAM DO SQLX
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
                    .pragma("cache_size", "-20000");

                let pool = sqlx::sqlite::SqlitePoolOptions::new()
                    .max_connections(5)
                    .connect_with(options)
                    .await
                    .expect("ERRO AO CONECTAR NO SQLITE: Verifique se o arquivo não está bloqueado ou corrompido.");

                sqlx::query("CREATE TABLE IF NOT EXISTS images (
                    id INTEGER PRIMARY KEY, 
                    name TEXT, 
                    path TEXT UNIQUE,
                    notes TEXT DEFAULT ''
                )")
                .execute(&pool).await.expect("Falha ao criar tabela imagens");

                sqlx::query("CREATE TABLE IF NOT EXISTS tags (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE
                )")
                .execute(&pool).await.expect("Falha ao criar tabela tags");

                sqlx::query("CREATE TABLE IF NOT EXISTS image_tags (
                    image_id INTEGER,
                    tag_id INTEGER,
                    PRIMARY KEY (image_id, tag_id),
                    FOREIGN KEY (image_id) REFERENCES images(id) ON DELETE CASCADE,
                    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
                )")
                .execute(&pool).await.expect("Falha ao criar tabela imagem_tags");

                sqlx::query("CREATE TABLE IF NOT EXISTS image_colors (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    image_id INTEGER NOT NULL,
                    hex_code TEXT NOT NULL,
                    r_val INTEGER NOT NULL,
                    g_val INTEGER NOT NULL,
                    b_val INTEGER NOT NULL,
                    weight REAL NOT NULL, -- Grau de dominância da cor (0.0 a 1.0)
                    FOREIGN KEY (image_id) REFERENCES images(id) ON DELETE CASCADE
                );")
                .execute(&pool).await.expect("Falha ao criar tabela imagem_cores");

                sqlx::query("CREATE INDEX IF NOT EXISTS idx_image_colors_rgb ON image_colors(r_val, g_val, b_val);")
                .execute(&pool).await.expect("Falha ao criar índice idx_image_colors_rgb");

                sqlx::query("CREATE TABLE IF NOT EXISTS moodboards (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    x_camera REAL NOT NULL DEFAULT 0,
                    y_camera REAL NOT NULL DEFAULT 0,
                    zoom_camera REAL NOT NULL DEFAULT 1,
                    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
                );")
                .execute(&pool).await.expect("Falha ao criar tabela moodboards");

                sqlx::query("CREATE TABLE IF NOT EXISTS moodboard_items (
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
                );")
                .execute(&pool).await.expect("Falha ao criar tabela moodboard_itens");

                sqlx::query("CREATE INDEX IF NOT EXISTS idx_moodboard_items_moodboard ON moodboard_items(moodboard_id);")
                .execute(&pool).await.expect("Falha ao criar idx_moodboard_itens_moodboard");

                sqlx::query("CREATE INDEX IF NOT EXISTS idx_moodboard_items_unique_type ON moodboard_items(unique_type);")
                .execute(&pool).await.expect("Falha ao criar idx_moodboard_items_unique_type");

                app.manage(AppState { db: pool });
            });

            let asset_scope = app.asset_protocol_scope();
            asset_scope.allow_directory("/", true).ok(); // Permite acesso à raiz no Linux
            
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            index_images,
            get_indexed_images,
            reset_index,
            generate_thumbnails_batch,
            open_original_image,
            get_indexed_folders,
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
            close_splashscreen])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
