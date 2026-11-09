import React, { useCallback, useMemo, useRef, useState } from "react";
import { useCatalogo } from "../context/CatalogoContext";

import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";

// Quantas pastas aparecem por vez na barra lateral.
const FOLDERS_PER_PAGE = 10;

interface FolderItemProps {
    path: string;
    isSelected: boolean;
    onSelect: (path: string) => void;
    onRemove: (path: string) => void;
}

// Linha de pasta memoizada: só re-renderiza quando o próprio `isSelected` muda.
const FolderItem = React.memo(function FolderItem({ path, isSelected, onSelect, onRemove }: FolderItemProps) {
    const folderName = path.split('/').pop();

    return (
        <div
            className={`folder-item ${isSelected ? 'active' : ''}`}
            title={path}
            onClick={() => onSelect(path)}
        >
            <span>{folderName}</span>

            <button
                className="btn-delete-folder"
                onClick={(e) => {
                    e.stopPropagation();
                    e.preventDefault();
                    onRemove(path);
                }}
            >
                remove
            </button>
        </div>
    );
});

export const SidebarLeft: React.FC = () => {
    const {
        folders,
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

    // Paginação e filtro das pastas ficam locais: `folders` continua sendo só um
    // array de strings no contexto, e apenas FOLDERS_PER_PAGE viram elementos no DOM.
    const [folderPage, setFolderPage] = useState(0);
    const [folderFilter, setFolderFilter] = useState("");

    const filteredFolders = useMemo(() => {
        const termo = folderFilter.trim().toLowerCase();
        if (!termo) return folders;
        return folders.filter(path => path.toLowerCase().includes(termo));
    }, [folders, folderFilter]);

    const totalFolderPages = Math.max(1, Math.ceil(filteredFolders.length / FOLDERS_PER_PAGE));
    // Se a lista encolher (remoção de pasta, filtro), a página atual é ajustada sozinha.
    const currentFolderPage = Math.min(folderPage, totalFolderPages - 1);

    const visibleFolders = useMemo(() => {
        const inicio = currentFolderPage * FOLDERS_PER_PAGE;
        return filteredFolders.slice(inicio, inicio + FOLDERS_PER_PAGE);
    }, [filteredFolders, currentFolderPage]);

    // Guarda a pasta selecionada em um ref para que os callbacks abaixo não
    // dependam dela (senão mudariam a cada seleção e anulariam o React.memo).
    const selectedFolderRef = useRef(selectedFolder);
    selectedFolderRef.current = selectedFolder;

    const handleAllImages = useCallback(() => {
        setSelectedFolder(null);
        setSelectedTag(null);
        setSelectedTags([]);
        setPage(0);
        setCatalogVersion(v => v + 1);
    }, [setSelectedFolder, setSelectedTag, setSelectedTags, setPage, setCatalogVersion]);

    // Selecionar uma pasta NÃO reindexa: as imagens já estão no banco e são
    // carregadas por página com get_indexed_images.
    const handleSelectFolder = useCallback((path: string) => {
        setSelectedTag(null);
        setSelectedTags([]);
        setSelectedFolder(path);
        setPage(0);
        setCatalogVersion(v => v + 1);
    }, [setSelectedTag, setSelectedTags, setSelectedFolder, setPage, setCatalogVersion]);

    const handleRemoveFolder = useCallback(async (path: string) => {
        const folderName = path.split('/').pop();

        const confirmar = await ask(
            `Deseja remover a indexação da pasta "${folderName}"?\nAs imagens físicas não serão apagadas.`,
            {
                title: "Confirmar Remoção",
                kind: "warning",
                okLabel: "Remover",
                cancelLabel: "Cancelar"
            }
        );

        if (!confirmar) return;

        try {
            await invoke("remove_indexed_folder", { folder: path });

            if (selectedFolderRef.current === path) {
                setSelectedFolder(null);
            }

            // Aqui a lista de pastas realmente mudou, então vale recarregar.
            const updated = await invoke<string[]>("get_indexed_folders");
            setFolders(updated);
            setCatalogVersion(v => v + 1);
            setPage(0);

        } catch (err) {
            console.error("Erro ao remover pasta:", err);
        }
    }, [setSelectedFolder, setFolders, setCatalogVersion, setPage]);

    return (
        <aside className="aside_left">
            <div className="aside_left_header">
                <h2 className="title">Folders</h2>

                <div
                    className={`folder-item ${selectedFolder === null ? 'active' : ''}`}
                    onClick={handleAllImages}
                >
                    <span>All Images</span>
                </div>
            </div>

            <div className="aside_left_content">
                {/* Com milhares de pastas, o filtro é o jeito mais rápido de chegar a uma delas */}
                <input
                    type="text"
                    className="folders_search"
                    placeholder="Filter folders..."
                    value={folderFilter}
                    onChange={(e) => {
                        setFolderFilter(e.target.value);
                        setFolderPage(0);
                    }}
                />

                <nav className="folders_nav">
                    {visibleFolders.map((path) => (
                        <FolderItem
                            key={path}
                            path={path}
                            isSelected={selectedFolder === path}
                            onSelect={handleSelectFolder}
                            onRemove={handleRemoveFolder}
                        />
                    ))}

                    {folders.length > 0 && filteredFolders.length === 0 && (
                        <span className="folders_empty">No folders found</span>
                    )}
                </nav>

                <div className="folders_pagination">
                    <button
                        disabled={currentFolderPage === 0}
                        onClick={() => setFolderPage(currentFolderPage - 1)}
                    >
                        ‹
                    </button>

                    <span>{currentFolderPage + 1} / {totalFolderPages}</span>

                    <button
                        disabled={currentFolderPage >= totalFolderPages - 1}
                        onClick={() => setFolderPage(currentFolderPage + 1)}
                    >
                        ›
                    </button>
                </div>

                {/* <hr className="divider" /> */}

                <h2 className="title">Tags</h2>
                {/* usar o input abaixo para FULL TEXT SEARCH no futuro */}
                {/* <input  type="text" placeholder="search tag"/> */}
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
            </div>

        </aside>
    )
}
