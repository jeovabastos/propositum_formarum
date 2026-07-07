import React, { useState, useEffect } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { useCatalogo } from '../context/CatalogoContext';

export const PlayerTreinoGestual: React.FC = () => {
  const { images, setStudyModeActive, page, setPage, LIMIT } = useCatalogo();

  const [indiceAtual, setIndiceAtual] = useState(0);
  const [tempoDefinido, setTempoDefinido] = useState(60);
  const [tempoRestante, setTempoRestante] = useState(tempoDefinido);
  const [pausado, setPausado] = useState(false);

  useEffect(() => {
    if (pausado || images.length === 0) return;

    if (tempoRestante <= 0) {
      proximaImagem();
      return;
    }

    const intervalo = setInterval(() => {
      setTempoRestante((tempo) => tempo - 1);
    }, 1000);

    return () => clearInterval(intervalo);
  }, [tempoRestante, pausado, images.length]);

  useEffect(() => {
    setTempoRestante(tempoDefinido);
  }, [indiceAtual, tempoDefinido]);

  const proximaImagem = () => {
    if (indiceAtual < images.length - 1) {
      setIndiceAtual((i) => i + 1);
    } 
    else if (images.length === LIMIT) {
      console.log("Fim do lote alcançado. Solicitando próxima página...");
      
      setPage(p => p + 1);
      setIndiceAtual(0);
    } 
    else {
      alert("Treino concluído! Você percorreu todas as referências disponíveis.");
      setStudyModeActive(false);
    }
  };

  const imagemAtual = images[indiceAtual];

  if (!imagemAtual && images.length === 0) {
    return (
      <div className="gestural-training-player player-carregando">
        <div className="loading-content">
          <span>🔄 Carregando próximo lote de poses...</span>
        </div>
      </div>
    );
  }

  if (!imagemAtual) return null;

  return (
    <div className="gestural-training-player">
      <div className="player-toolbar">
        <button onClick={() => setStudyModeActive(false)} className="btn-voltar">
          Sair do Modo Estudo (Pág. {page + 1})
        </button>
        
        <div className="timer-display">
          ⏱️ <span className={tempoRestante <= 10 ? "tempo-acabando" : ""}>{tempoRestante}s</span>
        </div>

        <div className="controles-midia">
          <button onClick={() => setPausado(!pausado)}>
            {pausado ? "▶️ Retomar" : "⏸️ Pausar"}
          </button>
          <button onClick={proximaImagem}>⏭️ Pular</button>
        </div>

        <select value={tempoDefinido} onChange={(e) => setTempoDefinido(Number(e.target.value))}>
          <option value={30}>30s (Rápido)</option>
          <option value={60}>1min</option>
          <option value={120}>2min</option>
          <option value={300}>5min (Longa)</option>
        </select>
      </div>

      <div className="player-image-container">
        <img 
          src={convertFileSrc(imagemAtual.path)} 
          alt={imagemAtual.path} 
          className="canvas-referencia"
        />
      </div>
    </div>
  );
};