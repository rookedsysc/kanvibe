import mermaid from "mermaid";

type RenderInput = { definition: string; theme: "light" | "dark"; width: number };
type RenderResult = { success: boolean; height?: number };
declare global {
  interface Window {
    diagramWorker: {
      receive: (callback: (input: RenderInput) => void) => void;
      complete: (result: RenderResult) => void;
    };
  }
}

window.diagramWorker.receive(async ({ definition, theme, width }) => {
  try {
    document.documentElement.style.cssText = "margin:0;padding:0;overflow:hidden";
    document.body.style.cssText = `margin:0;padding:8px;box-sizing:border-box;width:${width}px;background:${theme === "dark" ? "#111216" : "#ffffff"};overflow:hidden`;
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: theme === "dark" ? "dark" : "default",
      maxTextSize: 65536,
      maxEdges: 500,
      suppressErrorRendering: true,
      secure: ["secure", "securityLevel", "startOnLoad", "maxTextSize", "maxEdges", "suppressErrorRendering", "theme", "themeVariables", "themeCSS"],
    });
    const { svg } = await mermaid.render("kanvibeDiagram", definition);
    const container = document.getElementById("diagram")!;
    container.innerHTML = svg;
    const element = container.querySelector("svg");
    if (!element) throw new Error("Missing SVG");
    const viewBox = element.viewBox.baseVal;
    const natural = element.getBoundingClientRect();
    const naturalWidth = viewBox.width || natural.width;
    const naturalHeight = viewBox.height || natural.height;
    if (!Number.isFinite(naturalWidth) || !Number.isFinite(naturalHeight) || naturalWidth <= 0 || naturalHeight <= 0) throw new Error("Invalid SVG bounds");
    const scale = Math.min((width - 16) / naturalWidth, 2384 / naturalHeight);
    const actualWidth = Math.max(1, Math.floor(naturalWidth * scale));
    const actualHeight = Math.max(1, Math.floor(naturalHeight * scale));
    element.removeAttribute("style");
    element.style.cssText = `display:block;margin:0 auto;width:${actualWidth}px;height:${actualHeight}px;max-width:none`;
    element.setAttribute("width", String(actualWidth));
    element.setAttribute("height", String(actualHeight));
    await document.fonts.ready;
    // Offscreen Desktop compositor가 SVG 프레임을 만든 뒤 native 캡처를 요청한다.
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    const height = Math.min(2400, Math.ceil(container.getBoundingClientRect().height) + 16);
    window.diagramWorker.complete({ success: true, height });
  } catch {
    window.diagramWorker.complete({ success: false });
  }
});
