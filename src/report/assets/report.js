// Remembers which changes the streamer ticked as done and keeps the count.
// Ticking is a note for the streamer only: nothing here talks to OBS or Windows.
(function () {
  "use strict";

  var boxes = Array.prototype.slice.call(document.querySelectorAll("input[data-advice]"));
  if (!boxes.length) {
    return;
  }

  var KEY = "obs-doctor.listos";
  var revision = document.body.getAttribute("data-revision") || "";
  var chips = document.querySelectorAll(".chip");
  var count = document.querySelector("[data-count]");
  var allDone = document.querySelector("[data-alldone]");
  var noStore = document.querySelector("[data-nostore]");

  // Browsers often block storage for pages opened from disk, and every access
  // can throw. The page has to work the same without it.
  function read() {
    try {
      var saved = JSON.parse(window.localStorage.getItem(KEY) || "null");
      // Ticks belong to one check-up. If a newer report still lists a change,
      // it is not fixed, so it must not show up already ticked.
      if (saved && saved.revision === revision && Array.isArray(saved.done)) {
        return saved.done;
      }
    } catch (e) {
      /* no storage: start with nothing ticked */
    }
    return [];
  }

  function write(done) {
    try {
      window.localStorage.setItem(KEY, JSON.stringify({ revision: revision, done: done }));
      return true;
    } catch (e) {
      return false;
    }
  }

  function paint() {
    var done = 0;
    boxes.forEach(function (box, i) {
      var card = box.closest(".advice");
      if (card) {
        card.classList.toggle("is-done", box.checked);
      }
      if (chips[i]) {
        chips[i].classList.toggle("is-done", box.checked);
      }
      if (box.checked) {
        done += 1;
      }
    });
    if (count) {
      count.textContent = done + " de " + boxes.length + (boxes.length === 1 ? " hecho" : " hechos");
      count.hidden = false;
    }
    if (allDone) {
      allDone.hidden = done !== boxes.length;
    }
  }

  var remembered = read();
  boxes.forEach(function (box) {
    box.checked = remembered.indexOf(box.getAttribute("data-advice")) !== -1;
    box.addEventListener("change", function () {
      var done = boxes
        .filter(function (b) {
          return b.checked;
        })
        .map(function (b) {
          return b.getAttribute("data-advice");
        });
      if (!write(done) && noStore) {
        noStore.hidden = false;
      }
      paint();
    });
  });
  paint();
})();
