// Deteksi tinggi viewport sempit (mis. window minimum Tauri 860x600).
// Dipakai untuk memadatkan HUD agar transkrip tetap punya ruang baca.

import { useEffect, useState } from "react";

export function useCompactHeight(maxHeight = 720): boolean {
  const query = `(max-height: ${maxHeight}px)`;
  const [compact, setCompact] = useState(
    () => typeof window !== "undefined" && window.matchMedia(query).matches,
  );

  useEffect(() => {
    const list = window.matchMedia(query);
    const onChange = (event: MediaQueryListEvent) => setCompact(event.matches);
    setCompact(list.matches);
    list.addEventListener("change", onChange);
    return () => list.removeEventListener("change", onChange);
  }, [query]);

  return compact;
}
