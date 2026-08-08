import { TableScene } from "./table";

/** main.ts へ結線せず、呼ばれたときだけ確認用の卓を表示する。 */
export function mountDemo(container: HTMLElement): () => void {
  const canvas = document.createElement("canvas");
  container.appendChild(canvas);

  const scene = new TableScene(canvas);
  scene.showDemoHand();

  const resize = () => {
    scene.resize(container.clientWidth, container.clientHeight);
    scene.render();
  };
  resize();
  addEventListener("resize", resize);

  return () => {
    removeEventListener("resize", resize);
    scene.dispose();
    canvas.remove();
  };
}
