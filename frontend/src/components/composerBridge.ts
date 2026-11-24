// Jembatan kecil untuk mengisi textarea composer dari komponen lain
// (palette, quickstart). Composer sendiri yang memegang state teksnya.

export const COMPOSER_SET_EVENT = "maicli:composer-set";

export function setComposerText(text: string): void {
  window.dispatchEvent(new CustomEvent(COMPOSER_SET_EVENT, { detail: { text } }));
}
