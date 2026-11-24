// Optional enhancement: pages work without it.
// - refreshes departure lists that carry data-refresh (every data-every seconds, default 60)
// - runs the board clock from the server's time, so a kiosk with a wrong clock still shows the right time
// - on network failure keeps the last list and shows the offline note
// - remembers the last trips searched, on this device only (localStorage), and lists them on the home page
(function () {
  "use strict";
  var list = document.getElementById("deps");
  var clock = document.getElementById("clock");
  var offline = document.getElementById("offline");

  if (clock && clock.dataset.now) {
    var skew = Date.parse(clock.dataset.now) - Date.now();
    var tick = function () {
      var d = new Date(Date.now() + skew);
      clock.textContent = d.getHours() + ":" + String(d.getMinutes()).padStart(2, "0");
    };
    tick();
    setInterval(tick, 1000);
  }

  // ----- departures: a bus that has left drives off, the rest glide up -----
  var still = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
  var keyOf = function (li) { return li.getAttribute("data-key") || li.textContent; };
  var EASE = "cubic-bezier(0.16, 1, 0.3, 1)";
  function swap(list, html) {
    var old = {};
    Array.prototype.forEach.call(list.children, function (li) {
      old[keyOf(li)] = { el: li, top: li.getBoundingClientRect().top };
    });
    var next = document.createElement("ol");
    next.innerHTML = html;
    var keep = {};
    Array.prototype.forEach.call(next.children, function (li) { keep[keyOf(li)] = true; });
    var gone = Object.keys(old).filter(function (k) { return !keep[k]; });
    var changed = gone.length > 0 || next.children.length !== list.children.length;
    if (still || !list.animate || !changed) {
      if (list.innerHTML !== next.innerHTML) list.innerHTML = html;
      return;
    }
    // 1. the buses that left drive away to the left
    var away = gone.map(function (k) {
      return old[k].el.animate(
        [{ transform: "none", opacity: 1 }, { transform: "translateX(-45%)", opacity: 0 }],
        { duration: 550, easing: "cubic-bezier(0.55, 0, 0.75, 0.2)", fill: "forwards" }
      ).finished;
    });
    Promise.all(away).catch(function () {}).then(function () {
      // 2. the new list; rows that were already there glide from where they were
      list.innerHTML = html;
      Array.prototype.forEach.call(list.children, function (li, i) {
        var was = old[keyOf(li)];
        if (was) {
          var dy = was.top - li.getBoundingClientRect().top;
          if (Math.abs(dy) > 1) {
            li.animate([{ transform: "translateY(" + dy + "px)" }, { transform: "none" }],
              { duration: 650, easing: EASE });
          }
        } else {
          // 3. a new bus at the bottom fades in
          li.animate([{ opacity: 0, transform: "translateY(0.75rem)" }, { opacity: 1, transform: "none" }],
            { duration: 500, delay: 250 + 40 * i, easing: EASE, fill: "backwards" });
        }
      });
    });
  }

  if (list && list.dataset.refresh) {
    var url = list.dataset.refresh;
    var every = (parseInt(list.dataset.every, 10) || 60) * 1000;
    var keepQuery = new URLSearchParams(location.search).get("now");
    if (keepQuery) url += (url.indexOf("?") < 0 ? "?" : "&") + "now=" + encodeURIComponent(keepQuery);
    setInterval(function () {
      fetch(url, { cache: "no-store" })
        .then(function (r) { if (!r.ok) throw new Error(r.status); return r.text(); })
        .then(function (html) {
          swap(list, html);
          if (offline) offline.hidden = true;
        })
        .catch(function () { if (offline) offline.hidden = false; });
    }, every);
    list.swap = function (html) { swap(list, html); }; // for checks and demos
    // A kiosk running for weeks: reload once a night to pick up new code and feed.
    if (document.body.classList.contains("board")) {
      setTimeout(function () { location.reload(); }, 6 * 3600 * 1000);
    }
  }

  // ----- line timetable: open at the next trip, not at dawn -----
  document.querySelectorAll(".grid-wrap").forEach(function (wrap) {
    var col = wrap.querySelector("thead th.now-col");
    var first = wrap.querySelector("tbody th");
    if (col && first) wrap.scrollLeft = Math.max(0, col.offsetLeft - first.offsetWidth - 8);
  });

  // ----- recent trips -----
  var KEY = "recent-trips";
  var read = function () {
    try { return JSON.parse(localStorage.getItem(KEY)) || []; } catch (e) { return []; }
  };
  var params = new URLSearchParams(location.search);
  var from = params.get("from"), to = params.get("to");
  if (from && to && document.querySelector(".journey")) {
    var trips = read().filter(function (t) { return !(t.from === from && t.to === to); });
    trips.unshift({ from: from, to: to });
    try { localStorage.setItem(KEY, JSON.stringify(trips.slice(0, 5))); } catch (e) { /* private mode */ }
  }
  var box = document.getElementById("recent");
  var saved = read();
  if (box && saved.length) {
    var ul = box.querySelector("ul");
    var lang = params.get("lang");
    saved.forEach(function (t) {
      var a = document.createElement("a");
      a.href = "/?from=" + encodeURIComponent(t.from) + "&to=" + encodeURIComponent(t.to) + (lang ? "&lang=" + lang : "");
      a.innerHTML =
        '<span class="ic"><svg class="i" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" ' +
        'stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5"/>' +
        '<path d="M12 7.5V12l3 2"/></svg></span><span class="ends-txt"><span></span><span></span></span>';
      var names = a.querySelectorAll(".ends-txt span");
      names[0].textContent = t.from;
      names[1].textContent = "→ " + t.to;
      var li = document.createElement("li");
      li.appendChild(a);
      ul.appendChild(li);
    });
    box.hidden = false;
  }
})();
