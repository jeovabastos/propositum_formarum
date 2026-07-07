import "./App.css";
import { MyVirtualGrid } from "./components/MyVirtualGrid.tsx";
import { SidebarLeft } from "./components/SidebarLeft.tsx";
import { Toolbar } from "./components/Toolbar.tsx";
import { AsideRight } from "./components/AsideRight.tsx";
import { useCatalogo } from "./context/CatalogoContext.tsx";
import {PlayerTreinoGestual} from "./components/PlayerTreinoGestual.tsx"
import { MoodboardView } from "./components/MoodboardView.tsx"; 
import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

export const App: React.FC = () => {
  const [isReady, setIsReady] = useState(false);
  
  const inicializadoRef = useRef(false);

  useEffect(() => {
    if (inicializadoRef.current) return;
    inicializadoRef.current = true;

    const inicializarAplicativo = async () => {
      try {
        console.log("🚀 Frontend iniciado. Segurando a cortina por 2.5 segundos...");
        
        await new Promise(resolve => setTimeout(resolve, 2500));
        
      } catch (error) {
        console.error("Erro na inicialização:", error);
      } finally {
        console.log("⚙️ Enviando sinal para fechar a splash e mostrar a main...");
        setIsReady(true);
        await invoke('close_splashscreen');
      }
    };

    inicializarAplicativo();
  }, []);

  if (!isReady) {
    return <div style={{ background: "#121212", width: "100vw", height: "100vh" }} />;
  }





  const { studyModeActive, moodboardActive } = useCatalogo();

  if (studyModeActive) {
    return <PlayerTreinoGestual />;
  }

  if (moodboardActive) {
    return <MoodboardView/>;
  }

  return (
    <main className="main">
      <SidebarLeft />

      <section className="content">
        <Toolbar />
        <MyVirtualGrid />
      </section>

      <AsideRight />
    </main>
  );
};
