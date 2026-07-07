import { useState, useEffect } from 'react';
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useCatalogo } from "../context/CatalogoContext";

export const MyVirtualGrid: React.FC = () => {
  const {
      images,
      selectedImage, 
      setSelectedImage,
      multiselectionMode,
      selectedImagesIds,
      handleToogleSelection,
    } = useCatalogo();
    
  const [thumbnails, setThumbnails] = useState<Record<number, string>>({});
  const [carregandoLote, setCarregandoLote] = useState<boolean>(false);

  useEffect(() => {
    if (images.length === 0) {
      setThumbnails({});
      return;
    }

    async function processarMiniaturasDoLote() {
      setCarregandoLote(true);
      try {
        const caminhosThumb: string[] = await invoke("generate_thumbnails_batch", { 
          caminhos: images.map(img => img.path) 
        });

        const novoMapaThumbs: Record<number, string> = {};
        images.forEach((img, index) => {
          if (caminhosThumb[index]) {
            novoMapaThumbs[img.id] = convertFileSrc(caminhosThumb[index]);
          }
        });

        setThumbnails(novoMapaThumbs);
      } catch (err) {
        console.error("Erro ao gerar lote de miniaturas:", err);
      } finally {
        setCarregandoLote(false);
      }
    }

    processarMiniaturasDoLote();
  }, [images]);

  return (
    <div className='my_virtual_grid_container'>
      
      {carregandoLote && (
        <div className='loading_my_virtual_grid'>
          Indexando miniaturas do lote...
        </div>
      )}

      <div className='my_virtual_grid'>
        {images.map((img) => {
          const isSelected = multiselectionMode ? (selectedImagesIds && selectedImagesIds.includes(img.id)) : selectedImage?.id === img.id;

          const thumbSrc = thumbnails[img.id];

          return (
            <div 
              key={img.id} 
              className={`image_container ${isSelected ? 'selected' : ''}`}
    
                onClick={(e) => { 
                  e.stopPropagation(); 
                  console.log("Modo Multiseleção Ativo?", multiselectionMode);
                  console.log("Array atual de IDs:", selectedImagesIds);

                  if (multiselectionMode) {
                        handleToogleSelection(img.id);
                      } else {
                        setSelectedImage(img);
                      } 
                }}
                onDoubleClick={() => { invoke("open_original_image", { path: img.path }).catch(console.error); }}
            >
              {thumbSrc ? (
                <img 
                  src={thumbSrc} 
                  alt={img.name} 
                  loading="lazy"
                  decoding="async"
                  className='image_card'
                />
              ) : (
                <div className='image_card_placeholder'>
                  <span className='image_card_placeholder_span'>Carregando...</span>
                </div>
              )}
          
            </div>
          );
        })}
      </div>
    </div>
  );
}