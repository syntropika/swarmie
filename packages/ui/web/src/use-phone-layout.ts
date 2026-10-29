import { useSyncExternalStore } from "react";

const query = "(max-width: 767px)";

function subscribe(changed: () => void) {
  const media = window.matchMedia(query);
  media.addEventListener("change", changed);
  return () => {
    media.removeEventListener("change", changed);
  };
}

function snapshot() {
  return window.matchMedia(query).matches;
}

function serverSnapshot() {
  return false;
}

export function usePhoneLayout() {
  return useSyncExternalStore(subscribe, snapshot, serverSnapshot);
}
