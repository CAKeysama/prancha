(function () {
  var root = document.documentElement;
  try {
    var pref = localStorage.getItem("prancha.theme") || "dark";
    var dark = pref === "system" ? window.matchMedia("(prefers-color-scheme: dark)").matches : pref !== "light";
    root.dataset.theme = dark ? "dark" : "light";
    root.dataset.font = localStorage.getItem("prancha.font") || "hanken";
  } catch (e) {
    root.dataset.theme = "dark";
    root.dataset.font = "hanken";
  }
})();
