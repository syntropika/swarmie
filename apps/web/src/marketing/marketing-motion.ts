import { useEffect } from "react";

const entranceEase = "cubic-bezier(0.23, 1, 0.32, 1)";
const travelEase = "cubic-bezier(0.77, 0, 0.175, 1)";

function part(scene: Element, name: string): Element | null {
  return scene.querySelector(`[data-motion-part="${name}"]`);
}

/** Reveal the relationship inside each example while keeping SSR content readable. */
export function useMarketingMotion() {
  useEffect(() => {
    if (!("IntersectionObserver" in window) || !("animate" in Element.prototype)) {
      return undefined;
    }

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (reducedMotion.matches) {
      return undefined;
    }

    const running = new Set<Animation>();
    const handlers = new Map<Element, () => void>();

    function play(
      target: Element | null,
      frames: Keyframe[],
      duration: number,
      delay = 0,
      easing = entranceEase,
    ): Animation | undefined {
      if (!target) {
        return undefined;
      }
      const animation = target.animate(frames, { duration, delay, easing, fill: "backwards" });
      running.add(animation);
      const release = () => {
        running.delete(animation);
      };
      animation.addEventListener("finish", release, { once: true });
      animation.addEventListener("cancel", release, { once: true });
      return animation;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            observer.unobserve(entry.target);
            handlers.get(entry.target)?.();
          }
        }
      },
      { rootMargin: "0px 0px -45% 0px", threshold: 0 },
    );

    function watch(selector: string, onEnter: (scene: Element, index: number) => void) {
      for (const [index, scene] of document.querySelectorAll(selector).entries()) {
        handlers.set(scene, () => {
          onEnter(scene, index);
        });
        observer.observe(scene);
      }
    }

    watch('[data-motion-scene="hero"]', (scene) => {
      play(
        scene,
        [
          { opacity: 0.8, transform: "translateY(20px) scale(0.97)" },
          { opacity: 1, transform: "translateY(0) scale(1)" },
        ],
        980,
      );
      play(
        part(scene, "hero-question"),
        [
          { opacity: 0.58, transform: "translateX(-22px) scale(0.97)" },
          { opacity: 1, transform: "translateX(0) scale(1)" },
        ],
        800,
        100,
        travelEase,
      );
      play(
        part(scene, "hero-bridge"),
        [
          { opacity: 0.4, transform: "scaleX(0.05)" },
          { opacity: 1, transform: "scaleX(1)" },
        ],
        900,
        900,
        travelEase,
      );
      play(
        part(scene, "hero-context"),
        [
          { opacity: 0.65, transform: "scale(0.92)" },
          { opacity: 1, transform: "scale(1)" },
        ],
        760,
        1050,
        travelEase,
      );
      play(
        part(scene, "hero-answer"),
        [
          { opacity: 0.5, transform: "translateX(24px) scale(0.97)" },
          { opacity: 1, transform: "translateX(0) scale(1)" },
        ],
        900,
        1760,
        travelEase,
      );
      play(
        part(scene, "hero-decision"),
        [
          { opacity: 0.62, transform: "translateY(18px)" },
          { opacity: 1, transform: "translateY(0)" },
        ],
        820,
        2730,
        travelEase,
      );
    });

    watch('[data-motion-scene="problem"]', (scene) => {
      const starts = ["translate(-24px, -8px)", "translate(24px, -8px)", "translateY(24px)"];
      for (const [index, window] of scene
        .querySelectorAll('[data-motion-part="problem-window"]')
        .entries()) {
        play(
          window,
          [
            { opacity: 0.48, transform: starts[index] },
            { opacity: 1, transform: "translate(0)" },
          ],
          820,
          index * 120,
        );
      }
      play(
        part(scene, "problem-links"),
        [
          { opacity: 0.25, clipPath: "inset(0 100% 0 0)" },
          { opacity: 1, clipPath: "inset(0 0 0 0)" },
        ],
        1000,
        520,
        travelEase,
      );
    });

    watch('[data-motion-scene="onboarding"]', (scene) => {
      play(
        part(scene, "onboarding-question"),
        [
          { opacity: 0.55, transform: "translateX(-24px) scale(0.97)" },
          { opacity: 1, transform: "translateX(0) scale(1)" },
        ],
        820,
        0,
        travelEase,
      );
      play(
        part(scene, "onboarding-context-label"),
        [
          { opacity: 0.5, transform: "translateY(10px)" },
          { opacity: 1, transform: "translateY(0)" },
        ],
        700,
        900,
        travelEase,
      );
      const sources = part(scene, "onboarding-sources")?.querySelectorAll("li") ?? [];
      for (const [index, source] of sources.entries()) {
        play(
          source,
          [
            { opacity: 0.72, transform: "translateY(10px)" },
            { opacity: 1, transform: "translateY(0)" },
          ],
          600,
          1040 + index * 75,
          travelEase,
        );
      }
      play(
        part(scene, "onboarding-answer"),
        [
          { opacity: 0.5, transform: "translateX(24px) scale(0.97)" },
          { opacity: 1, transform: "translateX(0) scale(1)" },
        ],
        920,
        1810,
        travelEase,
      );
      play(
        part(scene, "onboarding-suggestion"),
        [
          { opacity: 0.58, transform: "translateY(30px) scale(0.96)" },
          { opacity: 1, transform: "translateY(0) scale(1)" },
        ],
        980,
        2800,
        travelEase,
      );
    });

    watch('[data-motion-scene="factory-step"]', (scene, index) => {
      if (index === 0) {
        play(
          part(scene, "factory-report-signal"),
          [
            { opacity: 0.38, transform: "translateY(16px)" },
            { opacity: 1, transform: "translateY(0)" },
          ],
          900,
          0,
          travelEase,
        );
      }
      if (index === 1) {
        play(
          part(scene, "factory-route-direct"),
          [
            { opacity: 0.38, transform: "translateY(14px)" },
            { opacity: 1, transform: "translateY(0)" },
          ],
          800,
          0,
          travelEase,
        );
        play(
          part(scene, "factory-route-team"),
          [
            { opacity: 0.38, transform: "translateY(-14px)" },
            { opacity: 1, transform: "translateY(0)" },
          ],
          800,
          360,
          travelEase,
        );
        play(
          part(scene, "factory-route-result"),
          [
            { opacity: 0.38, transform: "translateX(-18px)" },
            { opacity: 1, transform: "translateX(0)" },
          ],
          900,
          1050,
          travelEase,
        );
      }
      if (index === 2) {
        play(
          part(scene, "factory-pod-work"),
          [
            { opacity: 0.35, transform: "translateY(16px)" },
            { opacity: 1, transform: "translateY(0)" },
          ],
          960,
          250,
          travelEase,
        );
      }
      if (index === 3) {
        play(
          part(scene, "factory-review-handoff"),
          [
            { opacity: 0.4, transform: "translateX(-18px)" },
            { opacity: 1, transform: "translateX(0)" },
          ],
          900,
          200,
          travelEase,
        );
      }
    });

    function finishOnReducedMotion() {
      if (reducedMotion.matches) {
        observer.disconnect();
        for (const animation of running) {
          animation.finish();
        }
      }
    }

    reducedMotion.addEventListener("change", finishOnReducedMotion);
    return () => {
      observer.disconnect();
      reducedMotion.removeEventListener("change", finishOnReducedMotion);
      for (const animation of running) {
        animation.cancel();
      }
    };
  }, []);
}
