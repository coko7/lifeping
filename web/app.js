"use strict";

(() => {
  const STRINGS = {
    en: {
      "headline.loading": "Loading…",
      "headline.green": "Alive and well",
      "headline.yellow": "Probably fine, quiet for a while",
      "headline.red": "No sign of life for over {hours} hours",
      "headline.unknown": "No ping yet",
      lastSeen: "Last seen {relative}",
      "history.title": "Recent pings",
      "history.empty": "Nothing here yet",
      "footer.updated": "Updated {relative}",
      "error.unreachable": "Can't reach the server, retrying…",
    },
    fr: {
      "headline.loading": "Chargement…",
      "headline.green": "En vie et en forme",
      "headline.yellow": "Sûrement rien de grave, pas de nouvelles depuis un moment",
      "headline.red": "Aucun signe de vie depuis plus de {hours} heures",
      "headline.unknown": "Aucun ping pour l'instant",
      lastSeen: "Vu pour la dernière fois {relative}",
      "history.title": "Pings récents",
      "history.empty": "Rien pour l'instant",
      "footer.updated": "Mis à jour {relative}",
      "error.unreachable": "Serveur injoignable, nouvel essai…",
    },
  };

  const LANGS = ["en", "fr"];
  const STORAGE_KEY = "lifeping.lang";
  const POLL_MS = 30_000;
  const TICK_MS = 10_000;
  const FETCH_TIMEOUT_MS = 10_000;

  const TITLE_EMOJI = { loading: "⚪", unknown: "⚪", green: "🟢", yellow: "🟡", red: "🔴" };
  const FAVICON_COLOR = {
    loading: "#9ca3af",
    unknown: "#9ca3af",
    green: "#16a34a",
    yellow: "#d97706",
    red: "#dc2626",
  };

  const $ = (id) => document.getElementById(id);
  const el = {
    card: $("card"),
    headline: $("headline"),
    lastSeen: $("last-seen"),
    lastSeenExact: $("last-seen-exact"),
    notice: $("notice"),
    noticeText: $("notice-text"),
    historyTitle: $("history-title"),
    historyList: $("history-list"),
    historyEmpty: $("history-empty"),
    updated: $("updated"),
    favicon: $("favicon"),
    langButtons: document.querySelectorAll("[data-lang]"),
  };

  let lang = initialLang();
  let formatters = makeFormatters(lang);
  let data = null; // last successful /api/status response
  let offset = 0; // server clock minus local clock, in ms
  let lastSuccess = null; // local timestamp of the last successful fetch
  let failing = false;
  let pollTimer = null;

  // ---- i18n ---------------------------------------------------------------

  function initialLang() {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (LANGS.includes(stored)) return stored;
    } catch {
      // Storage unavailable (private mode, disabled cookies): fall through.
    }
    const preferred = navigator.languages?.length ? navigator.languages : [navigator.language];
    for (const tag of preferred) {
      const primary = String(tag || "").toLowerCase().split("-")[0];
      if (LANGS.includes(primary)) return primary;
    }
    return "en";
  }

  function setLang(next) {
    if (!LANGS.includes(next) || next === lang) return;
    lang = next;
    formatters = makeFormatters(lang);
    try {
      localStorage.setItem(STORAGE_KEY, lang);
    } catch {
      // Not persisted; the choice still applies to this visit.
    }
    render();
  }

  function t(key, vars = {}) {
    const template = STRINGS[lang][key] ?? STRINGS.en[key] ?? key;
    return template.replace(/\{(\w+)\}/g, (match, name) => (name in vars ? vars[name] : match));
  }

  function makeFormatters(locale) {
    return {
      relative: new Intl.RelativeTimeFormat(locale, { numeric: "auto" }),
      absolute: new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }),
      number: new Intl.NumberFormat(locale, { maximumFractionDigits: 2 }),
    };
  }

  /** Relative time of `thenMs` as seen from `nowMs`, in the largest sensible unit. */
  function relative(thenMs, nowMs) {
    // Timestamps slightly in the future (clock skew) read as "now".
    const secs = Math.min(0, (thenMs - nowMs) / 1000);
    const abs = Math.abs(secs);
    const rtf = formatters.relative;
    if (abs < 60) return rtf.format(Math.trunc(secs), "second");
    if (abs < 3600) return rtf.format(Math.trunc(secs / 60), "minute");
    if (abs < 2 * 86400) return rtf.format(Math.trunc(secs / 3600), "hour");
    return rtf.format(Math.trunc(secs / 86400), "day");
  }

  // ---- status -------------------------------------------------------------

  /** Same rules as the server (src/status.rs). */
  function computeStatus(latestMs, nowMs, yellowMs, redMs) {
    if (latestMs === null) return "unknown";
    const age = nowMs - latestMs;
    if (age < yellowMs) return "green";
    if (age < redMs) return "yellow";
    return "red";
  }

  // ---- rendering ----------------------------------------------------------

  // Only touch the DOM when text changes, so the live region announces
  // real state changes rather than every re-render.
  function setText(node, text) {
    if (node.textContent !== text) node.textContent = text;
  }

  function faviconUrl(color) {
    const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'><circle cx='8' cy='8' r='7' fill='${color}'/></svg>`;
    return `data:image/svg+xml,${encodeURIComponent(svg)}`;
  }

  function render() {
    const nowMs = Date.now() + offset;

    document.documentElement.lang = lang;
    for (const button of el.langButtons) {
      button.setAttribute("aria-pressed", String(button.dataset.lang === lang));
    }

    let status = "loading";
    let latestMs = null;
    if (data) {
      latestMs = data.latest ? Date.parse(data.latest) : null;
      status = computeStatus(
        latestMs,
        nowMs,
        data.thresholds.yellow_after_secs * 1000,
        data.thresholds.red_after_secs * 1000,
      );
    }

    el.card.dataset.status = status;
    const hours = data ? formatters.number.format(data.thresholds.red_after_secs / 3600) : "";
    setText(el.headline, t(`headline.${status}`, { hours }));

    if (latestMs !== null) {
      setText(el.lastSeen, t("lastSeen", { relative: relative(latestMs, nowMs) }));
      setText(el.lastSeenExact, formatters.absolute.format(latestMs));
    } else {
      setText(el.lastSeen, "");
      setText(el.lastSeenExact, "");
    }

    const title = `${TITLE_EMOJI[status]} lifeping`;
    if (document.title !== title) document.title = title;
    const icon = faviconUrl(FAVICON_COLOR[status]);
    if (el.favicon.getAttribute("href") !== icon) el.favicon.setAttribute("href", icon);

    renderHistory(nowMs);

    el.notice.hidden = !failing;
    setText(el.noticeText, t("error.unreachable"));

    setText(
      el.updated,
      lastSuccess === null ? "" : t("footer.updated", { relative: relative(lastSuccess, Date.now()) }),
    );
  }

  function renderHistory(nowMs) {
    setText(el.historyTitle, t("history.title"));
    const history = data ? data.history : [];
    el.historyEmpty.hidden = !data || history.length > 0;
    setText(el.historyEmpty, t("history.empty"));

    const items = history.map((iso) => {
      const ms = Date.parse(iso);
      const li = document.createElement("li");
      const time = document.createElement("time");
      time.dateTime = iso;
      time.textContent = formatters.absolute.format(ms);
      const rel = document.createElement("span");
      rel.className = "rel";
      rel.textContent = relative(ms, nowMs);
      li.append(time, rel);
      return li;
    });
    el.historyList.replaceChildren(...items);
  }

  // ---- polling ------------------------------------------------------------

  async function refresh() {
    try {
      const options = { cache: "no-store", headers: { Accept: "application/json" } };
      if (typeof AbortSignal !== "undefined" && AbortSignal.timeout) {
        options.signal = AbortSignal.timeout(FETCH_TIMEOUT_MS);
      }
      const response = await fetch("/api/status", options);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const json = await response.json();
      const serverNow = Date.parse(json.now);
      offset = Number.isNaN(serverNow) ? 0 : serverNow - Date.now();
      data = json;
      lastSuccess = Date.now();
      failing = false;
    } catch {
      failing = true;
    }
    render();
  }

  function schedule() {
    clearTimeout(pollTimer);
    if (document.hidden) return;
    pollTimer = setTimeout(async () => {
      await refresh();
      schedule();
    }, POLL_MS);
  }

  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      clearTimeout(pollTimer);
    } else {
      refresh().then(schedule);
    }
  });

  for (const button of el.langButtons) {
    button.addEventListener("click", () => setLang(button.dataset.lang));
  }

  render();
  refresh().then(schedule);
  setInterval(render, TICK_MS);
})();
