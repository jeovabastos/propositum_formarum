import React, { createContext, useContext, useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface ImageEntry { id: number; name: string; path: string; notes: string}
export interface Tag { id: number; name: string; }

interface CatalogoContextType {
  loading: boolean;
  setLoading: React.Dispatch<React.SetStateAction<boolean>>;
  studyModeActive: boolean;
  setStudyModeActive: React.Dispatch<React.SetStateAction<boolean>>;
  message: string;
  setMessage: React.Dispatch<React.SetStateAction<string>>;
  selectedFolder: string | null;
  setSelectedFolder: React.Dispatch<React.SetStateAction<string | null>>;
  page: number;
  setPage: React.Dispatch<React.SetStateAction<number>>;
  images: ImageEntry[];
  setImages: React.Dispatch<React.SetStateAction<ImageEntry[]>>;
  folders: string[];
  setFolders: React.Dispatch<React.SetStateAction<string[]>>;
  selectedTag: string | null;
  setSelectedTag: React.Dispatch<React.SetStateAction<string | null>>;
  catalogVersion: number;
  setCatalogVersion: React.Dispatch<React.SetStateAction<number>>;
  selectedImage: ImageEntry | null;
  setSelectedImage: React.Dispatch<React.SetStateAction<ImageEntry | null>>;
  imageTags: Tag[];
  setImageTags: React.Dispatch<React.SetStateAction<Tag[]>>;
  newTagInput: string;
  setNewTagInput: React.Dispatch<React.SetStateAction<string>>;
  allTags: Tag[];
  setAllTags: React.Dispatch<React.SetStateAction<Tag[]>>;
  selectedTags: string[];
  setSelectedTags: React.Dispatch<React.SetStateAction<string[]>>;
  tagsToAdd: number[];
  setTagsToAdd: React.Dispatch<React.SetStateAction<number[]>>;
  multiselectionMode: boolean;
  setMultiselectionMode: React.Dispatch<React.SetStateAction<boolean>>;
  selectedImagesIds: number[];
  setSelectedImagesIds: React.Dispatch<React.SetStateAction<number[]>>;
  preloadReady: boolean;
  updateFolders: ()=> Promise<void>;
  loadAllTagsGlobal: ()=> Promise<void>;
  handleAddTag: (imageId: number) => Promise<void>;
  handleRemoveTag: (imageId: number, tagId: number) => Promise<void>;
  toggleTagFilter: (tagName: string) => void;
  handleToogleSelection: (id: number) => void;
  retrieveSuccessStates: () => Promise<void>;
  LIMIT: number;
  moodboardActive: boolean;
  setMoodboardActive: React.Dispatch<React.SetStateAction<boolean>>;
}

const CatalogoContext = createContext<CatalogoContextType | undefined>(undefined);

export const CatalogoProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState("");
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [images, setImages] = useState<ImageEntry[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [catalogVersion, setCatalogVersion] = useState(0);
  const [selectedImage, setSelectedImage] = useState<ImageEntry | null>(null);
  const [imageTags, setImageTags] = useState<Tag[]>([]);
  const [newTagInput, setNewTagInput] = useState("");
  const [allTags, setAllTags] = useState<Tag[]>([]);
  const [selectedTag, setSelectedTag] = useState<string | null>(null);
  const [selectedTags, setSelectedTags] = useState<string[]>([]);
  const [tagsToAdd, setTagsToAdd] = useState<number[]>([]);
  const [preloadReady, setPreloadPronto] = useState<boolean>(false);
  const [multiselectionMode, setMultiselectionMode] = useState<boolean>(false);
  const [selectedImagesIds, setSelectedImagesIds] = useState<number[]>([]);
  const [studyModeActive, setStudyModeActive] = useState(false);
  const LIMIT = 20;
  const [moodboardActive, setMoodboardActive] = useState(false);

  useEffect(() => {
    let isMounted = true;
    let timerPrefetch: ReturnType<typeof setTimeout>;

    async function loadImages() {
      try {
        let data: ImageEntry[] = [];
        if (selectedTags.length > 0) {
          data = await invoke<ImageEntry[]>("get_images_by_multiple_tags", { tags: selectedTags, page: page, limit: LIMIT });
          console.log("data iniciando...")
        } else {
          data = await invoke<ImageEntry[]>("get_indexed_images", { page: page, limit: LIMIT, folder: selectedFolder || undefined });
        }

        if (isMounted) {
          setImages(data);
          document.querySelector('.main-viewport')?.scrollTo(0, 0);
          setPreloadPronto(false);

          timerPrefetch = setTimeout(async () => {
            try {
              let proximosCaminhos: string[] = [];
              if (selectedTags.length > 0) {
                const proximasImagens = await invoke<ImageEntry[]>("get_images_by_multiple_tags", { tags: selectedTags, page: page + 1, limit: LIMIT });
                proximosCaminhos = proximasImagens.map(img => img.path);
              } else {
                const proximasImagens = await invoke<ImageEntry[]>("get_indexed_images", { page: page + 1, limit: LIMIT, folder: selectedFolder || undefined });
                proximosCaminhos = proximasImagens.map(img => img.path);
              }

              if (proximosCaminhos.length > 0 && isMounted) {
                await invoke("generate_thumbnails_batch", { caminhos: proximosCaminhos });
                if (isMounted) setPreloadPronto(true);
              }
            } catch (err) {
              console.error("Erro no pre-fetch:", err);
            }
          }, 1500);
        }
      } catch (err) {
        console.error("Erro ao carregar imagens:", err);
      }
    }

    loadImages();

    return () => {
      isMounted = false;
      if (timerPrefetch) clearTimeout(timerPrefetch);
    };
  }, [page, selectedFolder, selectedTags, catalogVersion]);

  useEffect(() => {

    const tratarTeclado = (evento: KeyboardEvent): void => {
      
      if (evento.key === 'Escape' || evento.key === 'Esc') {
        if (multiselectionMode) {
          setSelectedImagesIds([]);
          setMultiselectionMode(false);
        } else {
          setSelectedImage(null);
        }
      }

      const target = evento.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") {
        return; 
      }
      
      if (evento.key === "ArrowRight"){
        if (images.length === LIMIT) {
          setPage(p => p + 1);
        }
      }
      if (evento.key === "ArrowLeft") setPage(p => p > 0 ? p - 1 : 0);
    };

    window.addEventListener('keydown', tratarTeclado);
    return () => window.removeEventListener('keydown', tratarTeclado);
  }, [multiselectionMode, images, LIMIT]);

  useEffect(() => {
    invoke<string>("verify_and_update_paths")
      .then(() => invoke<string[]>("get_indexed_folders"))
      .then((dadosPastas) => setFolders(dadosPastas))
      .catch((err) => console.error("Erro na inicialização:", err));

    const unlisten = listen("indexacao-concluida", async () => {
      const dadosPastas = await invoke<string[]>("get_indexed_folders");
      setFolders(dadosPastas);
      setSelectedFolder(null);
      setPage(0);
      setCatalogVersion(v => v + 1);
      setMessage("Catálogo atualizado com sucesso!");
      setLoading(false);
    });

    return () => { unlisten.then(f => f()); };
  }, []);

  useEffect(() => {
    if (!selectedImage) { setImageTags([]); return; }
    async function carregarTagsAtuais() {
      try {
        const lista = await invoke<Tag[]>("list_tags_from_image", { imageId: selectedImage!.id });
        setImageTags(lista);
      } catch (err) { console.error(err); }
    }
    carregarTagsAtuais();
  }, [selectedImage]);

  const loadAllTagsGlobal = async () => {
    try {
      const lista = await invoke<Tag[]>("get_all_tags");
      setAllTags(!lista || lista.length === 0 ? [] : lista);
    } catch (err) { console.error(err); setAllTags([]); }
  };

  useEffect(() => { loadAllTagsGlobal(); }, [catalogVersion]);

  const updateFolders = async () => {
    const atualizadas = await invoke<string[]>("get_indexed_folders");
    setFolders(atualizadas);
  };

  const handleAddTag = async (idDaImagem: number) => {
    const nomeLimpo = newTagInput.trim().toLowerCase();
    if (!nomeLimpo) return;
    try {
      const tagId = await invoke<number>("create_tag", { name: nomeLimpo });
      await invoke("link_tag_image", { imageId: idDaImagem, tagId });
      setNewTagInput("");
      const listaAtualizada = await invoke<Tag[]>("list_tags_from_image", { imageId: idDaImagem });
      setImageTags(listaAtualizada);
      loadAllTagsGlobal();
    } catch (err) { console.error(err); }
  };

  const handleRemoveTag = async (imageId: number, tagId: number) => {
    try {
      const tagParaRemover = imageTags.find(t => t.id === tagId);
      await invoke("unlink_tag_image", { imageId, tagId });
      await invoke("delete_empty_tag", { tagId });
      setImageTags(prev => prev.filter(t => t.id !== tagId));
      await loadAllTagsGlobal();

      if (tagParaRemover) {
        setSelectedTags(prev => {
          const novoFiltro = prev.filter(nome => nome !== tagParaRemover.name);
          if (novoFiltro.length === 0) setPage(0);
          return novoFiltro;
        });
      }
    } catch (error) { console.error(error); }
  };

  const toggleTagFilter = (nomeTag: string) => {
    setSelectedFolder(null);
    setPage(0);
    setSelectedTags(prev => prev.includes(nomeTag) ? prev.filter(t => t !== nomeTag) : [...prev, nomeTag]);
  };

  const handleToogleSelection = (id: number) => {
    setSelectedImagesIds(prev => {
      const listaAtual = prev || [];
      return listaAtual.includes(id) ? listaAtual.filter(itemId => itemId !== id) : [...listaAtual, id];
    });
  };

  const retrieveSuccessStates = async () => {
    setTagsToAdd([]);
    setNewTagInput("");
    setSelectedImagesIds([]);
    setMultiselectionMode(false);
    setCatalogVersion(prev => prev + 1);
  };

return (
    <CatalogoContext.Provider value={{
      loading, setLoading, message, setMessage, selectedFolder, setSelectedFolder, page, setPage,
      images, setImages, 
      folders, setFolders, catalogVersion, setCatalogVersion, selectedImage, setSelectedImage,
      imageTags, setImageTags, newTagInput, setNewTagInput, allTags, setAllTags, selectedTag, setSelectedTag, selectedTags,studyModeActive, setStudyModeActive,
      setSelectedTags, tagsToAdd, setTagsToAdd, multiselectionMode, setMultiselectionMode,
      selectedImagesIds, setSelectedImagesIds, preloadReady, updateFolders, loadAllTagsGlobal,
      handleAddTag, handleRemoveTag, toggleTagFilter, handleToogleSelection, retrieveSuccessStates, LIMIT, moodboardActive, setMoodboardActive
    }}>
      {children}
    </CatalogoContext.Provider>
  );
};

export const useCatalogo = () => {
  const context = useContext(CatalogoContext);
  if (!context) throw new Error("useCatalogo must be used within a CatalogoProvider");
  return context;
};