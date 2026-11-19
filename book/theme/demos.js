// A demo's video and recording are under the site root; the embed is
// included from the Demos page and from a chapter a level down, so its
// paths are resolved here with mdBook's `path_to_root`.
document.addEventListener("DOMContentLoaded", () => {
  for (const video of document.querySelectorAll(".demo video[data-src]")) {
    video.src = path_to_root + video.dataset.src;
  }
  for (const link of document.querySelectorAll(".demo a[data-href]")) {
    link.href = path_to_root + link.dataset.href;
  }
});
