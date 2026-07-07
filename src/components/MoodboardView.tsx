import React, { useRef, useState, useEffect, useCallback } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { useCatalogo } from '../context/CatalogoContext';
import { MyVirtualGrid } from './MyVirtualGrid';
import { Toolbar } from './Toolbar';
import { SidebarLeft } from './SidebarLeft';
import { AsideRight } from './AsideRight';

interface MipmapPaths {
    original: string;
    mid: string;
    thumb: string;
}

interface Camera {
    x: number;
    y: number;
    zoom: number;
}

interface BaseItem {
    id: string;
    x: number;
    y: number;
    width: number;
    height: number;
}

interface MoodboardBanco {
    id: number;
    name: string;
    x_camera: number;
    y_camera: number;
    zoom_camera: number;
    created_at: string;
    updated_at: string;
}

interface ItemImage extends BaseItem {
    unique_type: 'image';
    properties: {
        id_image_catalog: number; 
    };
    mipmaps?: {
        thumb: HTMLImageElement | null;
        mid: HTMLImageElement | null;
        original: HTMLImageElement | null;
    };
    paths?: MipmapPaths;
}

interface ItemDrawing extends BaseItem {
    unique_type: 'drawing';
    properties: {
        path_string: string;      
        color: string;
        thickness: number;
        width_original?: number;  
        height_original?: number; 
    };
    pathObjeto?: Path2D;
}

type MoodboardItem = ItemImage | ItemDrawing;

interface SalvarMoodboardDTO {
    id: number | null;
    name: string;
    x_camera: number;
    y_camera: number;
    zoom_camera: number;
    items: Omit<MoodboardItem, 'mipmaps' | 'pathObjeto'>[];
}

export const MoodboardView: React.FC = () => {
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const [items, setItems] = useState<MoodboardItem[]>([]);
    const [camera, setCamera] = useState<Camera>({ x: window.innerWidth / 2, y: window.innerHeight / 2, zoom: 1 });
    const [selectedId, setSelectedId] = useState<string | null>(null);
    const [action, setAction] = useState<'none' | 'drag' | 'resize' | 'drawing' | 'erasing' | 'pan'>('none');
    const [moodboardActiveId, setMoodboardActiveId] = useState<number | null>(null);
    const [drawingMode, setDrawingMode] = useState<boolean>(false);
    const [currentlyPoints, setCurrentlyPoints] = useState<{ x: number; y: number }[]>([]);
    const [eraserMode, setEraserMode] = useState<boolean>(false);
    const [moodboardName, setMoodboardName] = useState<string>("My Moodboard");
    const [moodboardList, setMoodboardList] = useState<MoodboardBanco[]>()
    const [importMultipleImages, setImportMultipleImages] = useState<boolean>(false);
    const [showGrid, setShowGrid] = useState(true);
    const [showMenu, setShowMenu] = useState(false);

    const { setMoodboardActive, selectedImagesIds, setMultiselectionMode, setPage, setSelectedImagesIds } = useCatalogo();

    const startRef = useRef<{
        x: number;
        y: number;
        itemX?: number;
        itemY?: number;
        itemW?: number;
        itemH?: number;
    }>({ x: 0, y: 0 });

    const salvarMoodboard = async () => {
        try {
            const itensLimpos = items.map(item => {
                if (item.unique_type === 'image') {
                    const { mipmaps, ...resto } = item;   
                    return resto;
                } else {
                    const { pathObjeto, ...resto } = item;
                    return resto;
                }
            });

            const dto: SalvarMoodboardDTO = {
                id: moodboardActiveId,
                name: moodboardName,
                x_camera: camera.x,
                y_camera: camera.y,
                zoom_camera: camera.zoom,
                items: itensLimpos
            };

            if(dto){
                const novoId = await invoke<number>("save_moodboard", { dto });
                setMoodboardActiveId(novoId);
    
                listarMoodboards()
                console.log(`✅ Moodboard salvo com ID: ${novoId}`);
                alert("Moodboard salvo com sucesso!");
            }

        } catch (error) {
            console.error("Erro ao salvar moodboard:", error);
            alert("Erro ao salvar: " + error);
        }
    };

    const carregarImagensDosItens = async (itensImagens: ItemImage[]) => {
        const imagensCarregadas = await Promise.all(
            itensImagens.map(async (item) => {
                return new Promise<ItemImage>((resolve) => {
                    // Chama o novo comando Rust que traz/gera os 3 caminhos
                    invoke<MipmapPaths>("search_image_paths", { id: item.properties.id_image_catalog })
                        .then(paths => {
                            // Instancia IMEDIATAMENTE apenas a miniatura de baixa resolução (leve)
                            const thumbImg = new Image();
                            thumbImg.crossOrigin = "anonymous";
                            thumbImg.src = convertFileSrc(paths.thumb);

                            resolve({ 
                                ...item, 
                                paths, // Guarda as strings dos caminhos no item
                                mipmaps: {
                                    thumb: thumbImg,
                                    mid: null,       // Fica nulo até o usuário dar zoom próximo
                                    original: null   // Fica nulo até o usuário dar zoom máximo
                                }
                            });
                        })
                        .catch(err => {
                            console.error(`Erro ao processar mipmaps para o item ${item.id}:`, err);
                            resolve(item); 
                        });
                });
            })
        );

        // Atualiza o estado mantendo o esqueleto reativo pronto para o canvas
        setItems(prevItems => prevItems.map(prevItem => {
            const imgPronta = imagensCarregadas.find(ic => ic.id === prevItem.id);
            return imgPronta ? imgPronta : prevItem;
        }));
    };

    const abrirMoodboard = async (id: number) => {
        try {
            const [moodboard, itensDoBanco] = await invoke<[MoodboardBanco, MoodboardItem[]]>("get_moodboard_with_items", { id });

            setMoodboardName(moodboard.name);
            setCamera({ x: moodboard.x_camera, y: moodboard.y_camera, zoom: moodboard.zoom_camera });
            setMoodboardActiveId(moodboard.id);

            const itensParaCanvas: MoodboardItem[] = itensDoBanco.map((item) => {
                if (item.unique_type === 'drawing') {
                    return {
                        ...item,
                        pathObjeto: new Path2D(item.properties.path_string)
                    } as ItemDrawing;
                }

                if (item.unique_type === 'image') {
                    return {
                        ...item,
                        image: undefined 
                    } as ItemImage;
                }

                return item;
            });

            setItems(itensParaCanvas);

            const itensImagens = itensParaCanvas.filter(i => i.unique_type === 'image') as ItemImage[];
            if (itensImagens.length > 0) {
                carregarImagensDosItens(itensImagens); 
            }

            console.log("✅ Moodboard carregado com sucesso!");

        } catch (error) {
            console.error("Erro ao abrir moodboard:", error);
            alert("Erro ao abrir: " + error);
        }
    };

    const listarMoodboards = async () => {
        try {
            const moodboardsExistentes = await invoke<MoodboardBanco[]>("list_moodboards")

            setMoodboardList(moodboardsExistentes)
        } catch (error) {
            alert(error)
        }
    }

    const deletarMoodboard = async (id: number) => {
        try {
            await invoke("delete_moodboard", { id: id })
            listarMoodboards()
            setItems([])
            setMoodboardName("")
            setMoodboardActiveId(null)
        } catch (error) {
            alert("erro ao deletar moodboard")
        }
    }

    const novoMoodboard = async () => {
        setItems([])
        setMoodboardName("")
        setMoodboardActiveId(null)
    }

    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            if ((e.ctrlKey || e.metaKey) && e.key === 's') {
                e.preventDefault(); 
                salvarMoodboard();
            }

            if (e.key === 'Tab'){
                e.preventDefault();
                setShowMenu(prev => !prev);
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [items, camera, moodboardName, moodboardActiveId]); 

    useEffect(() => {
        listarMoodboards()
    }, [])

    const renderCanvas2D = useCallback(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;

        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.fillStyle = '#121212';
        ctx.fillRect(0, 0, canvas.width, canvas.height);

        const gridSize = 20 * camera.zoom;
        const ox = camera.x % gridSize;
        const oy = camera.y % gridSize;

        ctx.strokeStyle = 'rgba(255, 255, 255, 0.03)';
        ctx.lineWidth = 1;
        ctx.beginPath();
        for (let x = ox; x < canvas.width; x += gridSize) { ctx.moveTo(x, 0); ctx.lineTo(x, canvas.height); }
        for (let y = oy; y < canvas.height; y += gridSize) { ctx.moveTo(0, y); ctx.lineTo(canvas.width, y); }
        ctx.stroke();

        ctx.save();
        ctx.translate(camera.x, camera.y);
        ctx.scale(camera.zoom, camera.zoom);

        // LAZY RENDERING (Frustum Culling): Limites visíveis do viewport
        const limiteEsquerdo = -camera.x / camera.zoom;
        const limiteDireito = (canvas.width - camera.x) / camera.zoom;
        const limiteTopo = -camera.y / camera.zoom;
        const limiteBaixo = (canvas.height - camera.y) / camera.zoom;

        // Renderiza estritamente os itens/imagens do moodboard
        items.forEach(item => {
            if (item.unique_type === 'image' && item.mipmaps) {
                // PASSO A: Frustum Culling (Sua lógica existente perfeita)
                const visivelNaHorizontal = item.x + item.width >= limiteEsquerdo && item.x <= limiteDireito;
                const visivelNaVertical = item.y + item.height >= limiteTopo && item.y <= limiteBaixo;

                if (!visivelNaHorizontal || !visivelNaVertical) return;

                // PASSO B: Seleção de Mipmap Baseado no Zoom Dinâmico e Lazy Load
                const tamanhoNaTela = item.width * camera.zoom;
                let imagemParaDesenhar: HTMLImageElement | null = null;

                if (tamanhoNaTela < 300) {
                    // Caso 1: Muito longe (Zoom Out) -> Usa Thumbnail
                    if (!item.mipmaps.thumb && item.paths) {
                        item.mipmaps.thumb = new Image();
                        item.mipmaps.thumb.src = convertFileSrc(item.paths.thumb);
                    }
                    imagemParaDesenhar = item.mipmaps.thumb;
                } 
                else if (tamanhoNaTela < 1200) {
                    // Caso 2: Distância Média -> Usa Mid Resolution
                    if (!item.mipmaps.mid && item.paths) {
                        item.mipmaps.mid = new Image();
                        item.mipmaps.mid.src = convertFileSrc(item.paths.mid);
                    }
                    // Se o Mid carregou, usa ele. Senão, faz fallback esticando o Thumbnail temporariamente
                    imagemParaDesenhar = (item.mipmaps.mid && item.mipmaps.mid.complete) 
                        ? item.mipmaps.mid 
                        : item.mipmaps.thumb;
                } 
                else {
                    // Caso 3: Zoom Aproximado -> Carrega e exibe Alta Resolução (Original)
                    if (!item.mipmaps.original && item.paths) {
                        item.mipmaps.original = new Image();
                        item.mipmaps.original.src = convertFileSrc(item.paths.original);
                    }
                    // Cascata de Fallback: Original -> Mid -> Thumb
                    if (item.mipmaps.original && item.mipmaps.original.complete) {
                        imagemParaDesenhar = item.mipmaps.original;
                    } else if (item.mipmaps.mid && item.mipmaps.mid.complete) {
                        imagemParaDesenhar = item.mipmaps.mid;
                    } else {
                        imagemParaDesenhar = item.mipmaps.thumb;
                    }
                }

                // Desenha a melhor resolução disponível no momento
                if (imagemParaDesenhar && (imagemParaDesenhar.complete || imagemParaDesenhar.src)) {
                    ctx.drawImage(imagemParaDesenhar, item.x, item.y, item.width, item.height);
                }

                // Desenha a borda de seleção
                if (item.id === selectedId) {
                    ctx.strokeStyle = '#007bff';
                    ctx.lineWidth = 2 / camera.zoom;
                    ctx.strokeRect(item.x, item.y, item.width, item.height);
                }
            }
        });


        // Desenha todos os traços suavizados finalizados (CORRIGIDO)
        items.forEach(item => {
            if (item.unique_type === 'drawing') {
                const visivelNaHorizontal = item.x + item.width >= limiteEsquerdo && item.x <= limiteDireito;
                const visivelNaVertical = item.y + item.height >= limiteTopo && item.y <= limiteBaixo;

                if (!visivelNaHorizontal || !visivelNaVertical) return;

                ctx.save();
                ctx.translate(item.x, item.y);

                const escalaX = item.width / (item.properties.width_original || item.width);
                const escalaY = item.height / (item.properties.height_original || item.height);
                ctx.scale(escalaX, escalaY);

                ctx.strokeStyle = item.properties.color || '#ffffff';
                ctx.lineWidth = (item.properties.thickness || 2.5) / camera.zoom;
                ctx.lineCap = 'round';
                ctx.lineJoin = 'round';

                if (item.pathObjeto) {
                    ctx.stroke(item.pathObjeto); 
                }
                ctx.restore();

                if (item.id === selectedId) {
                    ctx.strokeStyle = '#007bff';
                    ctx.lineWidth = 2 / camera.zoom;
                    ctx.strokeRect(item.x, item.y, item.width, item.height);
                }
            }
        });

        // Desenha o rascunho fluido (Real-time Feedback)
        if (action === 'drawing' && currentlyPoints.length > 0) {
            ctx.save();
            ctx.strokeStyle = '#007bff'; 
            ctx.lineWidth = 2.5 / camera.zoom;
            ctx.lineCap = 'round';
            ctx.lineJoin = 'round';

            ctx.beginPath();
            currentlyPoints.forEach((p, index) => {
                if (index === 0) ctx.moveTo(p.x, p.y);
                else ctx.lineTo(p.x, p.y);
            });
            ctx.stroke();
            ctx.restore();
        }

        ctx.restore();
    }, [items, camera, selectedId, action, currentlyPoints]);

    useEffect(() => { renderCanvas2D(); }, [renderCanvas2D]);

    // handle resize
    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const resize = () => {
            canvas.width = window.innerWidth;
            canvas.height = window.innerHeight;
            renderCanvas2D();
        };
        resize();
        window.addEventListener('resize', resize);
        return () => window.removeEventListener('resize', resize);
    }, [renderCanvas2D]);

    // handle camera wheel
    useEffect(() => {
        const handleWheel = (e: WheelEvent) => {
            if (!e.ctrlKey) return;
            e.preventDefault();

            const factor = e.deltaY > 0 ? 0.9 : 1.1;
            const newZoom = Math.max(0.1, Math.min(5, camera.zoom * factor));

            const dz = newZoom / camera.zoom;
            const newX = e.clientX - (e.clientX - camera.x) * dz;
            const newY = e.clientY - (e.clientY - camera.y) * dz;

            setCamera({ x: newX, y: newY, zoom: newZoom });
        };

        window.addEventListener('wheel', handleWheel, { passive: false });
        return () => window.removeEventListener('wheel', handleWheel);
    }, [camera]);

    // handle camera zoom in / out
    const screenToLogical = (sx: number, sy: number) => {
        return {
            x: (sx - camera.x) / camera.zoom,
            y: (sy - camera.y) / camera.zoom,
        };
    };

    // "N" handles com mouse abaixo
    useEffect(() => {
        const handleGlobalMouseMove = (e: MouseEvent) => {
            if (action === 'none' || selectedId === null) return;

            const dx = (e.clientX - startRef.current.x) / camera.zoom;
            const dy = (e.clientY - startRef.current.y) / camera.zoom;

            if (action === 'drag') {
                setItems(prev => prev.map(i =>
                    i.id === selectedId
                        ? { ...i, x: (startRef.current.itemX ?? i.x) + dx, y: (startRef.current.itemY ?? i.y) + dy }
                        : i
                ));
            }
            else if (action === 'resize') {
                setItems(prev => prev.map(i => {
                    if (i.id !== selectedId) return i;
                    const larguraInicial = startRef.current.itemW ?? i.width;
                    const alturaInicial = startRef.current.itemH ?? i.height;
                    const proporcaoOriginal = larguraInicial / alturaInicial;

                    const novaLargura = Math.max(100, larguraInicial + dx);
                    return {
                        ...i,
                        width: novaLargura,
                        height: novaLargura / proporcaoOriginal,
                    };
                }));
            }
        };

        const handleGlobalMouseUp = () => {
            setAction('none');
        };

        if (action === 'drag' || action === 'resize') {
            window.addEventListener('mousemove', handleGlobalMouseMove);
            window.addEventListener('mouseup', handleGlobalMouseUp);
        }

        return () => {
            window.removeEventListener('mousemove', handleGlobalMouseMove);
            window.removeEventListener('mouseup', handleGlobalMouseUp);
        };
    }, [action, selectedId, camera.zoom]);

    const handleCanvasMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
        const { x: lx, y: ly } = screenToLogical(e.clientX, e.clientY);
        const posLogica = obterCoordenadasLogicas(e.clientX, e.clientY);

        // Botão do meio (scroll) sempre entra em pan
        if (e.button === 1) {
            setSelectedId(null);
            setAction('pan');
            startRef.current = { x: e.clientX, y: e.clientY };
            return;
        }

        if (eraserMode) {
            setAction('erasing');
            // Apaga imediatamente o que foi clicado no primeiro clique
            setItems(prev => prev.filter(item => {
                if (item.unique_type === 'drawing') {
                    return !checarColisaoComDesenho(posLogica.x, posLogica.y, item);
                }
                return true; // Se for imagem, mantém no canvas
            }));
            return;
        }

        if (drawingMode) {
            setAction('drawing');
            setCurrentlyPoints([posLogica]);
            return; // Bloqueia interações com itens do moodboard
        }

        if (selectedId !== null && e.button === 0) {
            const item = items.find(i => i.id === selectedId);
            if (item && (item.unique_type === 'image' || item.unique_type === 'drawing')) {
                const h = 8 / camera.zoom;
                if (lx >= item.x + item.width - h && lx <= item.x + item.width + h &&
                    ly >= item.y + item.height - h && ly <= item.y + item.height + h) {
                    setAction('resize');
                    startRef.current = { x: e.clientX, y: e.clientY, itemX: item.x, itemY: item.y, itemW: item.width, itemH: item.height };
                    return;
                }
            }
        }

        const clicouImagem = items.slice().reverse().find(item =>
            item.unique_type === 'image' &&
            lx >= item.x && lx <= item.x + item.width &&
            ly >= item.y && ly <= item.y + item.height
        );

        const clicouDesenho = items.slice().reverse().find(item =>
            item.unique_type === 'drawing' &&
            lx >= item.x && lx <= item.x + item.width &&
            ly >= item.y && ly <= item.y + item.height
        );

        if (clicouImagem || clicouDesenho) {
            const itemClicado = clicouImagem || clicouDesenho;

            if (!itemClicado) return;

            setSelectedId(itemClicado.id);
            setAction('drag');
            startRef.current = { x: e.clientX, y: e.clientY, itemX: itemClicado.x, itemY: itemClicado.y, itemW: itemClicado.width, itemH: itemClicado.height };
            return
        }

        if (!clicouImagem && !clicouDesenho) {
            setSelectedId(null);
            setAction('pan');
            startRef.current = { x: e.clientX, y: e.clientY };
        }
    };

    const handleCanvasMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
        const posLogica = obterCoordenadasLogicas(e.clientX, e.clientY);

        if (action === 'erasing' && eraserMode) {
            setItems(prev => prev.filter(item => {
                if (item.unique_type === 'drawing') {
                    return !checarColisaoComDesenho(posLogica.x, posLogica.y, item as ItemDrawing);
                }
                // Se for imagem, retorna true para ignorar a borracha e mantê-la no canvas
                return true;
            }));
            return;
        }

        if (action === 'drawing' && drawingMode) {
            setCurrentlyPoints(prev => [...prev, posLogica]);
            return;
        }

        // 3. Trava do Pan
        if (action !== 'pan') return;

        // 4. Lógica de Movimentação da Câmera
        const panDx = e.clientX - startRef.current.x;
        const panDy = e.clientY - startRef.current.y;
        setCamera(prev => ({ ...prev, x: prev.x + panDx, y: prev.y + panDy }));
        startRef.current = { x: e.clientX, y: e.clientY };
    };

    const handleCanvasMouseUp = () => {
        if (action === 'erasing') {
            setAction('none');
            return;
        }

        if (action === 'drawing' && currentlyPoints.length > 1) {
            const xs = currentlyPoints.map(p => p.x);
            const ys = currentlyPoints.map(p => p.y);

            const minX = Math.min(...xs);
            const maxX = Math.max(...xs);
            const minY = Math.min(...ys);
            const maxY = Math.max(...ys);

            const largura = maxX - minX;
            const altura = maxY - minY;

            // ✅ Path RELATIVO ao (minX, minY)
            let svgPathString = "";
            svgPathString += `M ${currentlyPoints[0].x - minX} ${currentlyPoints[0].y - minY} `;

            if (currentlyPoints.length < 3) {
                currentlyPoints.forEach((p, i) => {
                    if (i > 0) svgPathString += `L ${p.x - minX} ${p.y - minY} `;
                });
            } else {
                for (let i = 1; i < currentlyPoints.length - 1; i++) {
                    const xc = (currentlyPoints[i].x + currentlyPoints[i + 1].x) / 2;
                    const yc = (currentlyPoints[i].y + currentlyPoints[i + 1].y) / 2;
                    svgPathString += `Q ${currentlyPoints[i].x - minX} ${currentlyPoints[i].y - minY} ${xc - minX} ${yc - minY} `;
                }
                const ultimo = currentlyPoints[currentlyPoints.length - 1];
                svgPathString += `L ${ultimo.x - minX} ${ultimo.y - minY}`;
            }

            const novoDesenho: MoodboardItem = {
                id: Date.now().toString(),
                unique_type: 'drawing',
                x: minX,
                y: minY,
                width: largura > 0 ? largura : 2,
                height: altura > 0 ? altura : 2,
                properties: {
                    path_string: svgPathString,
                    color: '#ff0000',
                    thickness: 2.5,
                    width_original: largura,
                    height_original: altura
                },
                pathObjeto: new Path2D(svgPathString) 
            };

            setItems(prev => [...prev, novoDesenho]);
            setCurrentlyPoints([]);
            setAction('none');
            return;
        }

        if (action === 'pan') setAction('none');
    };

    const realizarDownsampling = (imgOriginal: HTMLImageElement, maxDimensao: number = 1080): HTMLImageElement | null => {
        // Se a imagem for pequena, retorna null para avisar que não precisa mexer
        if (imgOriginal.width <= maxDimensao || imgOriginal.height <= maxDimensao) {
            return null;
        }

        let novaLargura = imgOriginal.width;
        let novaAltura = imgOriginal.height;

        if (novaLargura > novaAltura) {
            novaAltura = (maxDimensao / novaLargura) * novaAltura;
            novaLargura = maxDimensao;
        } else {
            novaLargura = (maxDimensao / novaAltura) * novaLargura;
            novaAltura = maxDimensao;
        }

        const canvasOtimizador = document.createElement('canvas');
        canvasOtimizador.width = novaLargura;
        canvasOtimizador.height = novaAltura;
        const ctxOtimizador = canvasOtimizador.getContext('2d');

        if (ctxOtimizador) {
            ctxOtimizador.drawImage(imgOriginal, 0, 0, novaLargura, novaAltura);
            const novaImgOtimizada = new Image();
            novaImgOtimizada.src = canvasOtimizador.toDataURL('image/webp', 0.85);
            return novaImgOtimizada;
        }

        return null;
    };

    const importarImagem = async () => {
        const file = await open({ filters: [{ name: 'Imagens', extensions: ['jpg', 'jpeg', 'png', 'gif', 'webp'] }] });

        if (file && typeof file === 'string') {
            const url = convertFileSrc(file);
            const imgProvisoria = new Image();
            imgProvisoria.crossOrigin = "anonymous";

            const nomeArquivo = file.split('\\').pop() || file.split('/').pop() || 'imagem';

            try {
                const idImagemBanco = await invoke<number>("get_or_create_image", {
                    path: file,
                    name: nomeArquivo
                });

                // const mipmapPaths = await invoke<MipmapPaths>("process_image_mipmaps", { filePath: file });
                const mipmapPaths = await invoke<MipmapPaths>("search_image_paths", { id: idImagemBanco });

                console.log(`✅ Imagem com ID: ${idImagemBanco}`);

                imgProvisoria.onload = () => {
                    const imgOtimizada = realizarDownsampling(imgProvisoria, 1920);

                    const pixelsOriginais = imgProvisoria.width * imgProvisoria.height;
                    const ramOriginalMB = ((pixelsOriginais * 4) / (1024 * 1024)).toFixed(2);
                    console.log(`%c[Original] Dimensões: ${imgProvisoria.width}x${imgProvisoria.height} | RAM: ${ramOriginalMB} MB`, "color: #ff4444");

                    const adicionarAoCanvas = (imagemFinal: HTMLImageElement) => {
                        const ratio = imagemFinal.width / imagemFinal.height;
                        const w = 320;
                        const h = w / ratio;

                        // Calcula o centro da tela convertendo para coordenadas lógicas do canvas infinito
                        const cx = (-camera.x + window.innerWidth / 2 - w / 2) / camera.zoom;
                        const cy = (-camera.y + window.innerHeight / 2 - h / 2) / camera.zoom;

                        // Cria o item respeitando a tipagem unificada (ItemImagem)
                        const novoItemImagem: ItemImage = {
                            id: Date.now().toString(), // ⚡ ID agora como String para padronizar com desenhos
                            unique_type: 'image',
                            x: cx,
                            y: cy,
                            width: w,
                            height: h,
                            // O pulo do gato: os dados específicos da mídia entram aqui
                            properties: {
                                id_image_catalog: idImagemBanco  // ID do SQLite encapsulado
                            },
                            paths: mipmapPaths, // <--- Salvando os caminhos aqui
                            mipmaps: {
                                thumb: null, // O loop do canvas vai instanciar na primeira renderização de forma ultra rápida
                                mid: null,
                                original: null
                            }
                            // image: imagemFinal 
                        };

                        setItems(prev => [...prev, novoItemImagem]);
                    };

                    if (imgOtimizada) {
                        imgOtimizada.onload = () => adicionarAoCanvas(imgOtimizada);
                    } else {
                        adicionarAoCanvas(imgProvisoria);
                    }
                };
                imgProvisoria.src = url;

            } catch (error) {
                console.error("Erro ao obter/criar imagem:", error);
                alert("Erro: " + error);
            }
        }
    };

    const obterCoordenadasLogicas = (clientX: number, clientY: number) => {
        return {
            x: (clientX - camera.x) / camera.zoom,
            y: (clientY - camera.y) / camera.zoom
        };
    };

    const checarColisaoComDesenho = (px: number, py: number, desenho: ItemDrawing) => {
        const tolerancia = 8 / camera.zoom;

        return (
            px >= desenho.x - tolerancia &&
            px <= desenho.x + desenho.width + tolerancia &&
            py >= desenho.y - tolerancia &&
            py <= desenho.y + desenho.height + tolerancia
        );
    };

    const carregarImagemDoCatalogo = async (
        id: number,
        offset: number,
        cameraSnapshot: Camera
    ): Promise<ItemImage | null> => {
        try {
            // Rust processa e devolve a pirâmide de caminhos
            const paths = await invoke<MipmapPaths>("search_image_paths", { id });

            // Instancia a miniatura inicial para renderizar na tela
            const thumbImg = new Image();
            thumbImg.crossOrigin = "anonymous";
            thumbImg.src = convertFileSrc(paths.thumb);

            // Para calcular as dimensões proporcionais iniciais, usamos a thumb que carrega quase instantaneamente
            return await new Promise<ItemImage | null>((resolve) => {
                thumbImg.onload = () => {
                    const width = 320;
                    const height = width * (thumbImg.naturalHeight / thumbImg.naturalWidth);

                    const cx = (-cameraSnapshot.x + window.innerWidth / 2 - width / 2 + offset) / cameraSnapshot.zoom;
                    const cy = (-cameraSnapshot.y + window.innerHeight / 2 - height / 2 + offset) / cameraSnapshot.zoom;

                    resolve({
                        id: `${Date.now()}-${Math.random()}`,
                        unique_type: "image",
                        x: cx,
                        y: cy,
                        width,
                        height,
                        properties: { id_image_catalog: id },
                        paths,
                        mipmaps: {
                            thumb: thumbImg,
                            mid: null,
                            original: null
                        }
                    });
                };
                
                thumbImg.onerror = () => resolve(null);
            });
        } catch (error) {
            console.error(`Erro ao importar imagem ${id}:`, error);
            return null;
        }
    };

    useEffect(()=>{
        console.log("chamando handleResetStates")
        
        handleResetStates()
    }, [items])

    const handleResetStates = ()=>{
        setSelectedImagesIds([])
        setMultiselectionMode(false)  
        setImportMultipleImages(false)
    }

    const handleImportSelectedImages = async () => {
        const idsToImport = [...selectedImagesIds];

        console.log("IDSTOIMPORT: ", idsToImport)

        if (idsToImport.length === 0) {
            alert("Nenhuma imagem selecionada!");
            return;
        }

        const cameraSnapshot = {
            x: camera.x,
            y: camera.y,
            zoom: camera.zoom,
        };

        const promises = idsToImport.map((idStr, index) => {
            const id = Number(idStr);
            const offset = index * 40;
            return carregarImagemDoCatalogo(id, offset, cameraSnapshot);
        });

        console.log("PROMISES: ", promises)

        const resultados = await Promise.all(promises)

        console.log("RESULTADOS: ", resultados)

        const novosItens = resultados.filter((item): item is ItemImage => item !== null);

        setItems(prev => [...prev, ...novosItens]);
    };
    
    return (
        // quadro em si
        <div style={{ width: '100vw', height: '100vh', margin: 0, padding: 0, overflow: 'hidden', position: 'relative', background: '#121212' }}>
            
            {/* área desenhável / editável */}
            <canvas
                ref={canvasRef}
                onMouseDown={handleCanvasMouseDown}
                onMouseMove={handleCanvasMouseMove}
                onMouseUp={handleCanvasMouseUp}
                onMouseLeave={handleCanvasMouseUp}
                style={{ display: 'block', position: 'absolute', top: 0, left: 0, zIndex: 1, cursor: action === 'pan' ? 'grabbing' : 'default' }}
            />

            {/* menu de opções */}
            {!showMenu &&
                <div>
                    <div className='moodboard_menu'>
                        <div>Zoom Quadro: {(camera.zoom * 100).toFixed(0)}%</div>
                        
                        <button 
                            onClick={() =>{ 
                                setMoodboardActive(false)
                                setMultiselectionMode(false)
                                setPage(0)
                            }} 
                        >
                            Exit
                        </button>

                        <nav>
                            <h2>Moodboards existentes:</h2>
                            {moodboardList ? moodboardList.map(item => {
                                return <div className='saved_moodboard'>
                                    <p onClick={() => abrirMoodboard(Number(item.id))}>{item.name}</p>
                                    <button onClick={() => deletarMoodboard(item.id)}>delete</button>
                                </div>
                            }) : <p>No moodboards saved</p>}
                        </nav>

                        <div style={{ marginBottom: 12 }}>
                            <label style={{ display: 'block', marginBottom: 4, fontSize: 12 }}>
                                Moodboard Name
                            </label>
                            <input
                                type="text"
                                value={moodboardName}
                                onChange={(e) => setMoodboardName(e.target.value)}
                                style={{
                                    width: '100%',
                                    padding: 8,
                                    borderRadius: 4,
                                    border: '1px solid #444',
                                    background: '#222',
                                    color: '#fff',
                                    boxSizing: 'border-box'
                                }}
                            />
                        </div>

                        <button
                            onClick={salvarMoodboard}
                        >
                            Save Moodboard
                        </button>

                        <button
                            onClick={() => { novoMoodboard() }}
                        >
                            New Moodboard
                        </button>

                        <button
                            onClick={() => setCamera({ x: window.innerWidth / 2, y: window.innerHeight / 2, zoom: 1 })}
                        >
                        Reset Canvas
                        </button>

                        <button
                            onClick={importarImagem}
                        >
                        Import Single Image
                        </button>

                        <button
                            onClick={() => {
                                setEraserMode(false)
                                setDrawingMode(!drawingMode)
                            }}
                        >
                            {drawingMode ? 'Pen mode active' : 'Activate drawing'}
                        </button>

                        <button
                            onClick={() => {
                                setEraserMode(!eraserMode);
                                setDrawingMode(false);
                                setSelectedId(null);
                            }}
                        >
                            {eraserMode ? 'Eraser active' : 'Activate eraser'}
                        </button>

                    </div>


                    <div className='moodboard_menu_left'>

                        {/* botão de mostrar o grid */}
                        {showGrid ?
                        <button
                            onClick={()=>{
                                console.log("aaaaaaaaaaaaaaaaaaaaaaaaaaa")
                                console.log(importMultipleImages)
                                setImportMultipleImages(true)
                            }}
                        >
                            Open Image GRID
                        </button> : null
                        }


                        {/* botão de importar as imagens */}
                        {
                            importMultipleImages &&
                        <button
                            onClick={async () => {
                                await handleImportSelectedImages()
                                setShowGrid(true)
                                setImportMultipleImages(false)
                            }}
                        >
                            Import Selected Images
                        </button>
                        }

                        {/* grid em si */}
                        {importMultipleImages ? (
                            <div className='moodboard_menu_left_grid'>
                                <main className="main">
                                    <SidebarLeft />

                                    <section className="content">
                                        <Toolbar />
                                        <MyVirtualGrid />
                                    </section>

                                    <AsideRight />
                                </main>
                            </div>
                            ) : null}
                    </div>
                </div>
            }
            
        </div>
    );
};