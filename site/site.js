// Theme toggle, the self-advancing tour, OS-aware download button, copy buttons.
(function () {
  var root = document.documentElement;

  document.getElementById("theme").addEventListener("click", function () {
    var next = root.dataset.theme === "light" ? "dark" : "light";
    root.dataset.theme = next;
    localStorage.setItem("ferrum-theme", next);
  });

  // Tour: one screenshot after the other, like a short clip; a click picks one
  // and hovering pauses it.
  var tour = document.querySelector("[data-tour]");
  if (tour) {
    var tabs = Array.prototype.slice.call(tour.querySelectorAll("[data-tab]"));
    var ms = 6000;
    var current = 0;
    var timer = null;
    // ?still: no autoplay (for page screenshots).
    var reduce = matchMedia("(prefers-reduced-motion: reduce)").matches || /still/.test(location.search);
    tour.style.setProperty("--tour-ms", ms + "ms");
    var show = function (i) {
      current = (i + tabs.length) % tabs.length;
      tabs.forEach(function (t, j) {
        t.setAttribute("aria-selected", j === current ? "true" : "false");
        var fig = tour.querySelector('[data-fig="' + t.dataset.tab + '"]');
        fig.classList.toggle("active", j === current);
        // Restart the progress bar.
        var bar = t.querySelector(".bar");
        bar.style.animation = "none";
        void bar.offsetWidth;
        bar.style.animation = "";
      });
    };
    var play = function () {
      clearInterval(timer);
      if (!reduce) timer = setInterval(function () { show(current + 1); }, ms);
    };
    tabs.forEach(function (t, i) {
      t.addEventListener("click", function () { show(i); play(); });
    });
    tour.addEventListener("mouseenter", function () { clearInterval(timer); tour.classList.add("paused"); });
    tour.addEventListener("mouseleave", function () { tour.classList.remove("paused"); play(); });
    // Only run while the tour is on screen.
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (entries) {
        if (entries[0].isIntersecting) { show(current); play(); } else { clearInterval(timer); }
      }).observe(tour);
    } else {
      play();
    }
  }

  // Download button: the right file for this computer.
  var ua = navigator.userAgent;
  var os = /Windows/.test(ua) ? "windows" : /Mac OS X|Macintosh/.test(ua) ? "macos"
    : /Linux|X11/.test(ua) && !/Android/.test(ua) ? "linux" : null;
  if (os) {
    var card = document.querySelector('.dl[data-os="' + os + '"]');
    var hero = document.getElementById("hero-download");
    if (card && hero) {
      card.classList.add("current");
      var name = card.querySelector("h3").textContent.trim();
      hero.querySelector("span").textContent += " · " + name;
    }
  }

  document.querySelectorAll("[data-copy]").forEach(function (b) {
    b.addEventListener("click", function () {
      var text = b.parentNode.querySelector("code").textContent;
      navigator.clipboard.writeText(text).then(function () {
        b.style.color = "var(--good)";
        setTimeout(function () { b.style.color = ""; }, 1200);
      });
    });
  });
})();
