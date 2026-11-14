// Reveals each part of a page as it is scrolled to. If this script does not run, nothing is hidden: the hiding is
// tied to the class it sets. A reader who asks for less motion is left alone.
(() => {
  if (!("IntersectionObserver" in window) || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const parts = document.querySelectorAll(".band, .try-notes, .proof-steps > li, .try-steps > li");
  if (parts.length === 0) return;
  document.documentElement.classList.add("motion");
  const seen = new IntersectionObserver((entries) => {
    for (const e of entries) if (e.isIntersecting) { e.target.classList.add("in"); seen.unobserve(e.target); }
  }, { rootMargin: "0px 0px -8% 0px", threshold: 0.05 });
  parts.forEach((p) => seen.observe(p));
  // a part reached by a link or the keyboard is shown at once, whatever the scroll position says
  addEventListener("hashchange", () => document.querySelector(location.hash || "#none")?.closest(".band")?.classList.add("in"));
  addEventListener("focusin", (e) => e.target.closest?.(".band, .try-notes, li")?.classList.add("in"));
})();
