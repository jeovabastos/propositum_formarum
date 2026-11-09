import React from "react";
import { useCatalogo } from "../context/CatalogoContext";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

// O backend devolve `name` (antes a interface declarava `nome`, mas só o id era usado).
interface Tag {
  id: number;
  name: string;
}

// Payload do evento "index-progress" emitido por index_images.
interface IndexProgress {
  indexed: number;
  done: boolean;
}

export const Toolbar: React.FC = () => {
    const {
        images,
        LIMIT,
        page,
        loading,
        multiselectionMode,
        selectedImagesIds,
        setLoading,
        setMessage,
        setCatalogVersion,
        setPage,
        updateFolders,
        setSelectedTags,
        setSelectedFolder,
        setImages,
        setMultiselectionMode,
        setSelectedImagesIds,
        setSelectedImage,
        setFolders,
        setAllTags
    } = useCatalogo();


    async function handleScan() {
        const selected = await open({
            directory: true,
            multiple: false,
            title: "Select images folder",
        });

        if (!selected) return;

        setLoading(true);
        setMessage("Indexing files...");

        let unlisten: UnlistenFn | null = null;

        try {
            // O listener precisa ser registrado ANTES do invoke para não perder eventos.
            unlisten = await listen<IndexProgress>("index-progress", (event) => {
                setMessage(`Indexing files... ${event.payload.indexed} found`);
            });

            // index_images agora devolve só o total (não mais a lista de imagens).
            // As imagens são carregadas por página via get_indexed_images,
            // que é disparado pelo setCatalogVersion/setPage abaixo.
            const total = await invoke<number>("index_images", { path: selected });

            await updateFolders();

            try {
                const tagsInDatabase = await invoke<Tag[]>("get_all_tags");
                if (tagsInDatabase && tagsInDatabase.length > 0) {
                    await Promise.all(
                        tagsInDatabase.map(tag => invoke("delete_empty_tag", { tagId: tag.id }))
                    );
                }
            } catch (e) {
                console.warn("Aviso: Falha ao expurgar tags residuais no scan:", e);
            }

            setCatalogVersion(prev => prev + 1);
            setPage(0);
            setMessage(`${total} images indexed.`);

        } catch (error) {
            // Agora erros de gravação no banco chegam aqui (antes eram engolidos no Rust).
            console.error("Erro ao indexar:", error);
            setMessage(`Erro ao processar pasta: ${error}`);
        } finally {
            unlisten?.();
            setLoading(false);
        }
    }

    async function handleReset() {
        const { ask } = await import("@tauri-apps/plugin-dialog");

        const confirmation = await ask(
            `Do you want to remove all files?.`,
            {
                title: "Remove all files",
                kind: "warning",
                okLabel: "Remove",
                cancelLabel: "Cancel"
            }
        );

        // O 'ask' retorna um booleano
        if (confirmation) {
            try {
                await invoke("reset_index");
                setMessage("Índice limpo!");

                setSelectedTags([]);
                setSelectedFolder(null);
                setPage(0);
                setImages([]);
                setFolders([]);
                setCatalogVersion(v => v + 1);

                setTimeout(() => {
                    setAllTags([]);
                    setFolders([]);
                    setMessage("Índice limpo com sucesso!");
                }, 50);
            } catch (e) {
                console.error(e);
            }
        }
    }

    // `page` vira u32 no Rust: um valor negativo ou NaN faria o invoke falhar.
    function handlePageInput(e: React.ChangeEvent<HTMLInputElement>) {
        const value = parseInt(e.target.value, 10);
        setPage(Number.isNaN(value) ? 0 : Math.max(0, value - 1));
    }

    return (
        <div className="toolbar">
            <div className="toolbar_folders">
                <button disabled={loading} onClick={handleScan} className="toolbar_folders_button_import">
                    {loading ? "Processing..." : "Import Folder"}
                </button>

                <button onClick={handleReset} className="toolbar_folders_button_reset">Reset All</button>
            </div>

            <div className="toolbar_pagination">
                <button disabled={page === 0} onClick={() => setPage(p => p - 1)}>
                    Previous
                </button>

                <input
                    type="number"
                    min={1}
                    value={page + 1}
                    onChange={handlePageInput}
                />

                <button disabled={images.length < LIMIT} onClick={() => setPage(p => p + 1)}>
                    Next
                </button>
            </div>

            <div className="toolbar_multiselection">
                <button
                    className={`toolbar_multiselection_button ${multiselectionMode ? 'ativo' : ''}`}
                    onClick={() => {
                        setMultiselectionMode(!multiselectionMode);
                        setSelectedImagesIds([]); 
                        setSelectedImage(null);
                    }}
                >
                    {multiselectionMode ? "Cancel" : "Select Multiple"}
                </button>

                {multiselectionMode && (
                    <span className="toolbar_multiselection_span">
                        {selectedImagesIds.length} selected
                    </span>
                )}
            </div>

        </div>
        
    )
}
