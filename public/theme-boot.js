(function () {
  try {
    var pref = localStorage.getItem("prancha.theme") || "dark";
    var dark = pref === "system" ? window.matchMedia("(prefers-color-scheme: dark)").matches : pref !== "light";
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  } catch (e) {
    document.documentElement.dataset.theme = "dark";
  }
})();
