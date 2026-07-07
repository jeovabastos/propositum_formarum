import React from "react";
import { invoke } from "@tauri-apps/api/core";
import { useCatalogo } from "../context/CatalogoContext";
import { useEffect, useState } from "react";
import { ImageEntry } from "../context/CatalogoContext";

export const AsideRight: React.FC = () => {
  const {
    selectedImage,
    imageTags,
    newTagInput,
    handleAddTag,
    handleRemoveTag,
    multiselectionMode,
    selectedImagesIds,
    allTags,
    selectedTags, 
    tagsToAdd, 
    toggleTagFilter,
    retrieveSuccessStates,
    setImages,
    setNewTagInput,
    setSelectedImage,
    setPage,
    setCatalogVersion,
    setSelectedTags,
    setTagsToAdd,
    setStudyModeActive,
    setMoodboardActive
  } = useCatalogo();

  const [notaLocal, setNotaLocal] = useState("");


  useEffect(() => {
    if (selectedImage) {
      setNotaLocal(selectedImage.notes || "");
    }
  }, [selectedImage]);

  const salvarNotaNoBanco = async () => {
    console.log(selectedImage?.name)
    if (!selectedImage) return;

    try {
      await invoke("save_image_note", {
        id: selectedImage.id,
        newNote: notaLocal,
      });

      setImages((imagensAtuais: ImageEntry[]) =>
        imagensAtuais.map((img) =>
          img.id === selectedImage.id ? { ...img, notes: notaLocal } : img
        )
      );
      console.log("Nota salva com sucesso!");
    } catch (error) {
      console.error("Erro ao salvar nota:", error);
    }
  };

  function hexToRgb(hex: string) {
    const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);

    if (!result) return null;

    // Pegamos a partir do índice 1, ignorando o índice 0 (que tem o "#ed333b")
    return {
      r: parseInt(result[1], 16), // "ed" -> 237
      g: parseInt(result[2], 16), // "33" -> 51
      b: parseInt(result[3], 16)  // "3b" -> 59
    };
  }

  async function lidarComBuscaPorCor(hexSelecionado: string) {
    const rgb = hexToRgb(hexSelecionado);
    if (!rgb) return [];

    try {
      const imagensFiltradas = await invoke('search_images_by_color', {
        rTarget: rgb.r,
        gTarget: rgb.g,
        bTarget: rgb.b,
        limit: 50
      });

      console.log("imagensFiltradas: ", imagensFiltradas)
      setImages(imagensFiltradas as ImageEntry[]);

      return imagensFiltradas;
    } catch (erro) {
      console.error("Erro ao buscar por cor:", erro);
      return [];
    }
  }

  async function buscarSimilaresDestaImagem(imageId: number) {
    try {
      const hexCorPrincipal: string = await invoke('get_image_main_color', { imageId });
      console.log("hexCorPrincipal: ", hexCorPrincipal);

      await lidarComBuscaPorCor(hexCorPrincipal);

    } catch (erro) {
      console.warn("Não foi possível buscar similares:", erro);
    }
  }

  return (

    <aside className="aside_right">
      <h2 className="title">Tools</h2>

      {multiselectionMode ? (
        // ==========================================
        // TELA A: MODO MULTISELEÇÃO ATIVO
        // ==========================================
        selectedImagesIds.length > 0 ? (
          <div className="painel-detalhes-lote">

            <h4>ADICIONAR/REMOVER Tags:</h4>
            <form className="tag-form" onSubmit={(e) => e.preventDefault()}>
              <div className="aside_right_existing_tags_box">
                {allTags.map((tag) => {
                  const isChecked = tagsToAdd.includes(tag.id);
                  return (
                    <label key={tag.id} className="tags-lote">
                      <input
                        type="checkbox"
                        checked={isChecked}
                        onChange={() => {
                          setTagsToAdd(prev =>
                            isChecked ? prev.filter(id => id !== tag.id) : [...prev, tag.id]
                          );
                        }}
                      />
                      <span> {tag.name}</span>
                    </label>
                  );
                })}
              </div>

              <input
                type="text"
                placeholder="Criar nova tag para o lote..."
                value={newTagInput}
                onChange={(e) => setNewTagInput(e.target.value)}
                className="tag-input aside_right_new_tag"
              />

              <div className="aside_right_control_buttons">
                <button
                  type="button"
                  className="btn-add-tag-lote aside_right_control_buttons_add"
                  disabled={tagsToAdd.length === 0 && newTagInput.trim() === ""}
                  onClick={async () => {
                    try {
                      let idsDasTagsFinais = [...tagsToAdd];

                      if (newTagInput.trim() !== "") {
                        const idGerado = await invoke<number>("create_tag", { name: newTagInput.trim() });
                        if (idGerado) idsDasTagsFinais.push(idGerado);
                      }

                      await invoke("link_tags_and_images_in_bulk", {
                        imageIds: selectedImagesIds,
                        tagIds: idsDasTagsFinais.filter(id => id != null)
                      });

                      retrieveSuccessStates();
                    } catch (err) {
                      console.error(err);
                    }
                  }}
                >
                  Insert tags in the bulk
                </button>

                <button
                  type="button"
                  className="btn-remove-tag-lote aside_right_control_buttons_remove"
                  disabled={tagsToAdd.length === 0}
                  onClick={async () => {
                    try {
                      const nomesDasTagsRemovidas = allTags
                        .filter(tag => tagsToAdd.includes(tag.id))
                        .map(tag => tag.name);

                      await invoke("remove_tags_and_images_in_bulk", {
                        imageIds: selectedImagesIds,
                        tagIds: tagsToAdd
                      });

                      await Promise.all(
                        tagsToAdd.map(tagId => invoke("delete_empty_tag", { tagId }))
                      );

                      setSelectedTags((prevFiltros) => {
                        const novoFiltro = prevFiltros.filter(nome => !nomesDasTagsRemovidas.includes(nome));

                        if (novoFiltro.length === 0) {
                          setPage(0); 
                        }

                        return novoFiltro;
                      });

                      setCatalogVersion(prev => prev + 1);
                      retrieveSuccessStates();

                    } catch (err) {
                      console.error("Erro ao remover tags em lote:", err);
                      alert(`Erro: ${err}`);
                    }
                  }}
                >
                  Remove Tags from bulk
                </button>
              </div>
            </form>
          </div>
        ) : (
          <p className="placeholder-text">Select one or more images in the grid to apply tags in bulk.</p>
        )
      ) : (
        // ==========================================
        // TELA B: MODO INDIVIDUAL (Seu código atual)
        // ==========================================
        selectedImage ? (
            <div className="painel-detalhes">
              <div className="painel-detalhes-basico">
                <p>Name: {selectedImage.name}</p>

                <p className="caminho-texto" title={selectedImage.path}>
                  Path: {selectedImage.path}
                </p>

                <div className="painel-detalhes-basico-tags">
                <p>Tags:</p>
                  {imageTags.map((tag) => {
                    const isSelected = selectedTags.includes(tag.name);
                    
                    return (
                      <div>
                        <button
                          key={tag.id}
                          className={`booru-tag-button ${isSelected ? 'active-tag' : ''}`}
                          onClick={() => {
                            toggleTagFilter(tag.name);
                          }}
                        >
                          🏷️ {tag.name}
                        </button>

                        <button onClick={() => { handleRemoveTag(selectedImage.id, tag.id) }}>X</button>
                      </div>
                    );
                  })}
                </div>
              </div>

              <div>
                <form
                  onSubmit={async (e) => {
                    e.preventDefault();
                    if (newTagInput.trim() !== "") {
                      await handleAddTag(selectedImage.id);
                    }

                    if (tagsToAdd.length > 0) {
                      try {
                        await invoke("link_tags_in_bulk", {
                          imageId: selectedImage.id,
                          tagIds: tagsToAdd
                        });

                        imageTags.map((tag) => {

                          return (
                            <div key={tag.id}>{tag.name}</div>
                          )
                        })
                        setTagsToAdd([]);
                      } catch (err) {
                        console.error("Erro ao adicionar tags em lote:", err);
                      }
                    }
                  }}

                  className="painel-detalhes-form"
                >
                  <div className="painel-detalhes-form-tags">
                    <p className="option">Link existing tags:</p>

                    {allTags
                      .filter(t => !imageTags.some(imgTag => imgTag.id === t.id))
                      .map((tag) => {
                        const isChecked = tagsToAdd.includes(tag.id);

                        return (
                          <label key={tag.id} className="form-tag-label">
                            <input
                            className="form-tag-input"
                              type="checkbox"
                              checked={isChecked}
                              onChange={() => {
                                setTagsToAdd(prev =>
                                  isChecked ? prev.filter(id => id !== tag.id) : [...prev, tag.id]
                                );
                              }}
                            />
                            <span className="form-tag-span"> {tag.name}</span>
                          </label>
                        );
                      })}
                  </div>

                  <div className="painel-detalhes-form-tags-input">
                    <input
                      type="text"
                      placeholder="Ou crie uma nova tag..."
                      value={newTagInput}
                      onChange={(e) => setNewTagInput(e.target.value)}
                      className="tag-input"
                    />

                    <button type="submit" className="painel-detalhes-form-add-button">
                      Apply selected tags
                    </button>
                  </div>
                </form>
              </div>

              <div className="buscar-imagem-semelhante">
                <button onClick={() => buscarSimilaresDestaImagem(selectedImage.id)}>
                  🔍 Search for similar items by color
                </button>
              </div>

              <div className="notas-container">
                <label>Notas de Estudo:</label>
                <textarea
                  value={notaLocal}
                  onChange={(e) => setNotaLocal(e.target.value)}
                  onBlur={salvarNotaNoBanco} 
                  placeholder="Escreva aqui observações sobre anatomia, luz, pose..."
                  className="textarea-notas"
                />
              </div>

              <div className="fechar-detalhes">
                <button onClick={() => setSelectedImage(null)} className="btn-fechar">
                  Close details
                </button>
              </div>
            </div>
        ) : (
          <div className="aside-right-options-container">
            <p className="placeholder-text">Click on an image to manage the tags.</p>

            <button onClick={() => {
              setStudyModeActive(prev => !prev)
            }}>Study Mode
            </button>

            <button onClick={() => {
              setMoodboardActive(prev => !prev)
            }}>Moodboard
            </button>

            <div className="aside-right-color-picker">
              <label >Filter by color:</label>
              <input
                type="color"
                id="color-picker"
                onChange={(e) => lidarComBuscaPorCor(e.target.value)}
              />
            </div>
          </div>
        )
      )}
    </aside>

  )
}