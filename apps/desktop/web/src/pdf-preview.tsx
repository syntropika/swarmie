import type { PDFDocumentProxy, PDFDocumentLoadingTask, RenderTask } from "pdfjs-dist";
// Vite supplies a URL string default for ?url assets, independent from the underlying JavaScript exports.
// oxlint-disable-next-line import/default
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import { useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";

import { startAction, errorMessage } from "./action-boundary";

export function PdfPreview(props: { data: string; name: string }) {
  return <PdfDocument key={props.data} {...props} />;
}
function PdfDocument({ data, name }: { data: string; name: string }) {
  const container = useRef<HTMLDivElement>(null);
  const [document, setDocument] = useState<PDFDocumentProxy>();
  const [page, setPage] = useState(1);
  const [zoom, setZoom] = useState(1);
  const [width, setWidth] = useState(320);
  const [issue, setIssue] = useState("");
  useEffect(() => {
    let disposed = false;
    let loading: PDFDocumentLoadingTask | undefined;
    const load = async () => {
      try {
        const pdfjs = await import("pdfjs-dist/legacy/build/pdf.mjs");
        if (disposed) {
          return;
        }
        pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;
        loading = pdfjs.getDocument({
          data: Uint8Array.from(atob(data), (character) => character.codePointAt(0) ?? 0),
          useWasm: false,
        });
        const result = await loading.promise;
        if (!disposed) {
          setDocument(result);
        }
      } catch (error) {
        if (!disposed) {
          setIssue(`Cannot preview this PDF: ${errorMessage(error)}`);
        }
      }
    };
    startAction(load(), reportPdfFailure);
    return () => {
      disposed = true;
      if (loading !== undefined) {
        startAction(loading.destroy(), reportPdfFailure);
      }
    };
  }, [data]);
  useEffect(() => {
    const element = container.current;
    if (element === null) {
      return undefined;
    }
    const observer = new ResizeObserver(() => {
      setWidth(Math.max(160, element.clientWidth - 2));
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);
  return (
    <div className="pdf-preview" ref={container}>
      {issue.length > 0 && (
        <p className="inline-error" role="alert">
          {issue}
        </p>
      )}
      {document === undefined ? (
        issue.length === 0 && <p className="muted">Loading PDF…</p>
      ) : (
        <>
          <div className="permission-actions">
            <Button
              variant="ghost"
              size="sm"
              disabled={page === 1}
              onClick={() => {
                setPage(page - 1);
              }}
            >
              Previous page
            </Button>
            <span aria-live="polite">
              Page {page} of {document.numPages}
            </span>
            <Button
              variant="ghost"
              size="sm"
              disabled={page === document.numPages}
              onClick={() => {
                setPage(page + 1);
              }}
            >
              Next page
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={zoom <= 0.5}
              aria-label="Zoom PDF out"
              onClick={() => {
                setZoom(Math.max(0.5, zoom - 0.25));
              }}
            >
              −
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={zoom >= 3}
              aria-label="Zoom PDF in"
              onClick={() => {
                setZoom(Math.min(3, zoom + 0.25));
              }}
            >
              +
            </Button>
          </div>
          <PdfPage
            key={`${page}:${width}:${zoom}`}
            document={document}
            name={name}
            page={page}
            width={width}
            zoom={zoom}
          />
        </>
      )}
    </div>
  );
}
function reportPdfFailure(error: unknown) {
  // PDF resource disposal is independent from the node's execution state.
  console.error("PDF preview resource failed", errorMessage(error));
}
function PdfPage({
  document,
  name,
  page,
  width,
  zoom,
}: {
  document: PDFDocumentProxy;
  name: string;
  page: number;
  width: number;
  zoom: number;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [rendering, setRendering] = useState(true);
  const [issue, setIssue] = useState("");
  useEffect(() => {
    const target = canvas.current;
    if (target === null) {
      return undefined;
    }
    let disposed = false;
    let task: RenderTask | undefined;
    const render = async () => {
      try {
        const pdfPage = await document.getPage(page);
        if (disposed) {
          return;
        }
        const base = pdfPage.getViewport({ scale: 1 });
        const viewport = pdfPage.getViewport({ scale: Math.min(2, width / base.width) * zoom });
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
        if (!disposed) {
          setRendering(false);
        }
      } catch (error) {
        if (!disposed) {
          setRendering(false);
          setIssue(`Cannot render this PDF page: ${errorMessage(error)}`);
        }
      }
    };
    startAction(render(), reportPdfFailure);
    return () => {
      disposed = true;
      task?.cancel();
    };
  }, [document, page, width, zoom]);
  return (
    <>
      <div className="pdf-page" aria-busy={rendering}>
        <figure>
          <canvas ref={canvas} aria-label={`${name}, page ${page}`} />
          <figcaption className="sr-only">
            {name}, page {page}
          </figcaption>
        </figure>
      </div>
      {issue.length > 0 && (
        <p className="inline-error" role="alert">
          {issue}
        </p>
      )}
    </>
  );
}
