import { useEffect, useRef, useState } from "react";
import type { PDFDocumentProxy, RenderTask } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import { Button } from "@/components/ui/button";

export function PdfPreview({ data, name }: { data: string; name: string }) {
  const container = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [document, setDocument] = useState<PDFDocumentProxy>();
  const [page, setPage] = useState(1);
  const [zoom, setZoom] = useState(1);
  const [width, setWidth] = useState(320);
  const [error, setError] = useState("");
  const [rendering, setRendering] = useState(false);
  useEffect(() => {
    let disposed = false;
    let loading: import("pdfjs-dist").PDFDocumentLoadingTask | undefined;
    setDocument(undefined);
    setPage(1);
    setError("");
    void import("pdfjs-dist/legacy/build/pdf.mjs")
      .then(async (pdfjs) => {
        if (disposed) return;
        pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
        loading = pdfjs.getDocument({
          data: Uint8Array.from(atob(data), (character) =>
            character.charCodeAt(0),
          ),
          useWasm: false,
        });
        const result = await loading.promise;
        if (!disposed) setDocument(result);
      })
      .catch((failure) => {
        if (!disposed) setError(`Cannot preview this PDF: ${String(failure)}`);
      });
    return () => {
      disposed = true;
      void loading?.destroy();
    };
  }, [data]);
  useEffect(() => {
    const element = container.current;
    if (!element) return;
    const observer = new ResizeObserver(() =>
      setWidth(Math.max(160, element.clientWidth - 2)),
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const target = canvas.current;
    if (!document || !target) return;
    let disposed = false;
    let task: RenderTask | undefined;
    setRendering(true);
    setError("");
    void document
      .getPage(page)
      .then(async (pdfPage) => {
        if (disposed) return;
        const base = pdfPage.getViewport({ scale: 1 });
        const viewport = pdfPage.getViewport({
          scale: Math.min(2, width / base.width) * zoom,
        });
        const ratio = Math.min(2, window.devicePixelRatio || 1);
        target.width = Math.ceil(viewport.width * ratio);
        target.height = Math.ceil(viewport.height * ratio);
        target.style.width = `${viewport.width}px`;
        target.style.height = `${viewport.height}px`;
        task = pdfPage.render({
          canvas: target,
          viewport,
          transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0],
        });
        await task.promise;
        if (!disposed) setRendering(false);
      })
      .catch((failure) => {
        if (!disposed) {
          setRendering(false);
          setError(`Cannot render this PDF page: ${String(failure)}`);
        }
      });
    return () => {
      disposed = true;
      task?.cancel();
    };
  }, [document, page, width, zoom]);
  return (
    <div className="pdf-preview" ref={container}>
      {error ? (
        <p className="inline-error" role="alert">
          {error}
        </p>
      ) : !document ? (
        <p className="muted">Loading PDF…</p>
      ) : (
        <>
          <div className="permission-actions">
            <Button
              variant="ghost"
              className="small-button"
              disabled={page === 1}
              onClick={() => setPage(page - 1)}
            >
              Previous page
            </Button>
            <span aria-live="polite">
              Page {page} of {document.numPages}
            </span>
            <Button
              variant="ghost"
              className="small-button"
              disabled={page === document.numPages}
              onClick={() => setPage(page + 1)}
            >
              Next page
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              disabled={zoom <= 0.5}
              aria-label="Zoom PDF out"
              onClick={() => setZoom(Math.max(0.5, zoom - 0.25))}
            >
              −
            </Button>
            <Button
              variant="ghost"
              className="small-button"
              disabled={zoom >= 3}
              aria-label="Zoom PDF in"
              onClick={() => setZoom(Math.min(3, zoom + 0.25))}
            >
              +
            </Button>
          </div>
          <div className="pdf-page" aria-busy={rendering}>
            <canvas
              ref={canvas}
              role="img"
              aria-label={`${name}, page ${page}`}
            />
          </div>
        </>
      )}
    </div>
  );
}
