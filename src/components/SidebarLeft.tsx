import React from "react";
import { useCatalogo } from "../context/CatalogoContext";

import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";

export const SidebarLeft: React.FC = () => {
    const {
        folders,
        selectedTag,
        setSelectedTag,
        setSelectedTags,
        selectedFolder,
        setSelectedFolder,
        setPage,
        setFolders,
        setCatalogVersion,
        allTags,
        selectedTags,
        toggleTagFilter
    } = useCatalogo();

    return (
        <aside className="aside_left">
            <h2 className="title">Folders</h2>

            <div
                className={`folder-item ${selectedFolder === null ? 'active' : ''}`}
                onClick={async () => {
                    console.log(selectedTag)

                    const atualizadas = await invoke<string[]>("get_indexed_folders");
                    setFolders(atualizadas);
                    setCatalogVersion(v => v + 1);


                    setSelectedFolder(null);
                    setSelectedTag(null); 
                    setSelectedTags([])
                    setPage(0);
                }}
            >
                <span>All Images</span>
            </div>

            <nav>
                {folders.map((path) => {
                    const folderName = path.split('/').pop()
                    const isSelected = selectedFolder === path;

                    return (
                        <div key={path} className={`folder-item ${isSelected ? 'active' : ''}`} title={path} 
                        onClick={async () => {
                            try {
                                await invoke("index_images", { path: path });

                                const updated = await invoke<string[]>("get_indexed_folders");
                                setFolders(updated);
                                setCatalogVersion(v => v + 1);
                                setPage(0);

                            } catch (err) {
                                console.error("Erro ao remover pasta:", err);
                            }



                            setSelectedTag(null);
                            setSelectedTags([])
                            setSelectedFolder(path);
                            setPage(0);
                        }}><span>{folderName}</span>

                            <button
                                className="btn-delete-folder"
                                onClick={async (e) => {
                                    e.stopPropagation(); 
                                    e.preventDefault();  

                                    const confirmar = await ask(
                                        `Deseja remover a indexação da pasta "${folderName}"?\nAs imagens físicas não serão apagadas.`,
                                        {
                                            title: "Confirmar Remoção",
                                            kind: "warning",
                                            okLabel: "Remover",
                                            cancelLabel: "Cancelar"
                                        }
                                    );

                                    if (confirmar) {
                                        try {
                                            await invoke("remove_indexed_folder", { folder: path });

                                            if (selectedFolder === path) {
                                                setSelectedFolder(null);
                                            }

                                            const updated = await invoke<string[]>("get_indexed_folders");
                                            setFolders(updated);
                                            setCatalogVersion(v => v + 1);
                                            setPage(0);

                                        } catch (err) {
                                            console.error("Erro ao remover pasta:", err);
                                        }
                                    }
                                }}
                            >
                                remove
                            </button>


                        </div>
                    )
                })}
            </nav>

            <hr className="divider" />

            <h2 className="title">Global Tags</h2>
            <div className="booru-tags-container">
                {
                    allTags.map((tag) => {
                        const isSelected = selectedTags.includes(tag.name);

                        return (
                            <button
                                key={tag.id}
                                className={`booru-tag-button ${isSelected ? 'active-tag' : ''}`}
                                onClick={() => {
                                    toggleTagFilter(tag.name)
                                }}
                            >
                                {tag.name}
                            </button>
                        );
                    })
                }
            </div>
        </aside>
    )
}