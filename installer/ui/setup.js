"use strict";
const $ = (s) => document.querySelector(s);

const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
let speaker = null;
// Keep the wizard inside the viewport; its action stays visible when fields expand.
document.querySelectorAll(".wizard > .card").forEach(card => {
  const action = card.querySelector(".actions, .finish-actions, #scan");
  const body = document.createElement("div"), footer = document.createElement("div");
  body.className = "step-body"; footer.className = "step-footer";
  if (action) footer.append(action);
  body.append(...card.childNodes);
  card.append(body, footer);
});

async function api(method, url, body) {
  const r = await fetch(url, { method, headers: { "Content-Type": "application/json", "X-Token": window.TOKEN }, body: body ? JSON.stringify(body) : undefined });
  const d = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(d.error || `Ошибка ${r.status}`);
  return d;
}

function step(n) {
  [1, 2, 3, 4].forEach((i) => $("#s" + i).classList.toggle("hidden", i !== n));
  document.body.dataset.step = String(n);
  $(".steps").style.setProperty("--step", String(n - 1));
  const card = $("#s" + n);
  card.classList.remove("enter");
  void card.offsetWidth;
  card.classList.add("enter");
  card.querySelector(".step-body").scrollTop = 0;
  document.querySelectorAll(".steps li").forEach((li) => { const s = +li.dataset.s; li.className = s === n ? "on" : s < n ? "done" : ""; });
}

$("#ver").textContent = window.VERSION || "";
const TZ = [["Europe/Moscow", "Москва (UTC+3)"], ["Europe/Minsk", "Минск (UTC+3)"], ["Europe/Kaliningrad", "Калининград (UTC+2)"], ["Europe/Samara", "Самара (UTC+4)"],
  ["Asia/Yekaterinburg", "Екатеринбург (UTC+5)"], ["Asia/Almaty", "Алматы (UTC+5)"], ["Asia/Omsk", "Омск (UTC+6)"], ["Asia/Novosibirsk", "Новосибирск (UTC+7)"],
  ["Asia/Krasnoyarsk", "Красноярск (UTC+7)"], ["Asia/Irkutsk", "Иркутск (UTC+8)"], ["Asia/Yakutsk", "Якутск (UTC+9)"], ["Asia/Vladivostok", "Владивосток (UTC+10)"],
  ["Europe/Kyiv", "Киев"], ["Europe/Berlin", "Берлин"], ["UTC", "UTC"]];
TZ.sort((a, b) => (a[0] === "UTC" ? -1 : b[0] === "UTC" ? 1 : 0));
$("#tz").innerHTML = TZ.map(([v, n]) => `<option value="${v}">${n}</option>`).join("");


function speakerRow(sp, i) {
  const ok = !sp.problems?.length;
  const badge = !ok ? `<span class="badge bad">не подходит</span>` : sp.installed ? `<span class="badge up">Джарвис ${esc(sp.installed)}</span>` : `<span class="badge ok">можно ставить</span>`;
  const meta = [sp.ip, sp.free_mb ? `свободно ${sp.free_mb} МБ` : "", sp.ram_mb ? `${sp.ram_mb} МБ памяти` : ""].filter(Boolean).map(esc).join(" · ");
  return `<button type="button" class="sp${ok ? "" : " no"}" data-ip="${esc(sp.ip)}" style="--i:${i}"><span class="sp-ring${sp.installed ? " on" : ""}" aria-hidden="true"></span>
    <span class="b"><b>${esc(sp.model || "Колонка")}</b><small>${meta}</small></span>${badge}<span class="sp-go" aria-hidden="true">→</span></button>`;
}

$("#scan").addEventListener("click", async (e) => {
  $("#scan").disabled = true;
  $("#scan-st").innerHTML = `<span class="radar" aria-hidden="true"><i></i><i></i></span> Ищу колонки в домашней сети…`;
  $("#found").innerHTML = "";
  try {
    const d = await api("GET", "/api/scan");
    const list = d.speakers || [];
    $("#scan-st").textContent = list.length ? `Найдено: ${list.length}` : "Введите IP колонки, если она ещё не подключена по ADB.";
    $("#found").innerHTML = list.map(speakerRow).join("");
    list.forEach((s, i) => { const el = $("#found").children[i]; if (el) el.style.setProperty("--i", i); });
    window._list = list;
  } catch (err) { $("#scan-st").textContent = err.message; }
  $("#scan").disabled = false;
});
$("#found").addEventListener("click", (e) => { const row = e.target.closest(".sp"); if (row) show(window._list.find((s) => s.ip === row.dataset.ip)); });
$("#ip-go").addEventListener("click", async (e) => {
  $("#ip-go").disabled = true;
  try { show(await api("POST", "/api/check", { ip: $("#ip").value.trim() })); } catch (err) { alert(err.message); }
  $("#ip-go").disabled = false;
});

function tile(k, v, sub, pct, tone) {
  return `<div class="tile${tone ? " " + tone : ""}"><span class="k">${k}</span><span class="n">${v}</span>${pct != null ? `<span class="meter"><i style="--p:${Math.max(3, Math.min(100, pct))}%"></i></span>` : ""}<small>${sub}</small></div>`;
}
function drawSpace(sp) {
  const after = sp.staging_mb ?? sp.free_mb;
  const short = sp.space_ok === false;
  const need = sp.need_mb == null ? "Уточним после загрузки пакета" : `Нужно до ${sp.need_mb} МиБ`;
  $("#tile-space").outerHTML = tile("Место для установки", `${after}<small>МиБ</small>`, need,
    sp.need_mb == null ? null : after / Math.max(sp.need_mb * 2, after) * 100, short ? "bad" : "").replace('class="tile', 'id="tile-space" class="tile');
  $("#to3").disabled = !!sp.problems?.length || short;
}
function show(sp) {
  speaker = sp;
  $("#sp-title").textContent = sp.model || "Колонка";
  $("#sp-ip").textContent = `${sp.ip} · ${sp.installed ? "Джарвис " + sp.installed + " — обновим, настройки сохранятся" : "проверка устройства"}`;
  $("#sp-tiles").innerHTML = `<div id="tile-space"></div>` +
    tile("Память", `${sp.ram_mb || "?"}<small>МБ</small>`, sp.ram_mb && sp.ram_mb < 200 ? "впритык, но работать будет" : "хватает", null) +
    tile("Джарвис", sp.installed ? esc(sp.installed) : "—", sp.installed ? "будет обновлён" : "ещё не установлен", null);
  const checks = [
    [sp.root, "Root-доступ по ADB", sp.root ? "есть" : "нужна разлоченная колонка"],
    [!sp.android, "Прошивка на Linux", sp.android ? "Android-Станции пока не поддерживаются" : "подходит"],
    [!!sp.userland, "Микрофон Алисы", sp.userland ? `библиотека ${esc(sp.userland)}` : "не удалось определить"],
    [!(sp.problems || []).some((p) => /профил|init\.d|maind/.test(p)), "Автозапуск и Алиса", "найдены"],
  ];
  $("#sp-checks").innerHTML = checks.map(([ok, k, v], i) => `<li class="${ok ? "ok" : "bad"}" style="--i:${i}" title="${esc(v)}"><i aria-hidden="true"></i><span>${k}${ok ? "" : `<small>${v}</small>`}</span></li>`).join("");
  $("#sp-info").innerHTML = [["IP", sp.ip], ["Модель", sp.model || "?"], ["Процессор", sp.arch || "?"], ["Алиса / микрофон", sp.userland || "?"], ["Root", sp.root ? "есть" : "нет"], ["Память", sp.ram_mb ? sp.ram_mb + " МБ" : "?"], ["Свободно места", sp.free_mb ? sp.free_mb + " МБ" : "?"], ["Джарвис", sp.installed || "не установлен"]]
    .map(([k, v]) => `<span>${k}</span><span>${esc(v)}</span>`).join("");
  $("#sp-problems").innerHTML = (sp.problems || []).map((p) => `<div class="problem">${esc(p)}</div>`).join("") + (sp.notes || []).map((n) => `<div class="note">${esc(n)}</div>`).join("");
  drawSpace(sp);
  $("#to3").textContent = sp.installed ? "Обновить" : "Дальше";
  $("#uninstall").classList.toggle("hidden", !sp.installed);
  step(2);
}

document.querySelectorAll("[data-back]").forEach((b) => b.addEventListener("click", () => step(+b.dataset.back)));
$("#to3").addEventListener("click", () => {
  // при обновлении пароль и ключи уже на колонке — поля можно оставить пустыми
  $("#pw").placeholder = speaker.installed ? "Не менять" : "Пароль";
  $("#tz-keep")?.remove();
  if (speaker.installed) {
    const option = new Option("Оставить как есть", "", true, true);
    option.id = "tz-keep";
    $("#tz").prepend(option);
  }
  step(3);
});

$("#install").addEventListener("click", async () => {
  const pw = $("#pw").value;
  $("#s3-err").textContent = "";
  if (!speaker.installed && pw.length < 6) { $("#s3-err").textContent = "Придумайте пароль — минимум 6 символов"; return; }
  if (pw && pw !== $("#pw2").value) { $("#s3-err").textContent = "Пароли не совпадают"; return; }
  try {
    await api("POST", "/api/install", { ip: speaker.ip, password: pw, timezone: $("#tz").value });
    $("#pw").value = $("#pw2").value = "";
    run("Устанавливаю…");
  } catch (err) { $("#s3-err").textContent = err.message; }
});

$("#uninstall").addEventListener("click", async () => {
  if (!confirm("Удалить Джарвиса с колонки? Она перезагрузится, Алиса останется как была.")) return;
  try { await api("POST", "/api/uninstall", { ip: speaker.ip }); run("Удаляю…"); } catch (err) { alert(err.message); }
});

const STAGES = [["Подключение", /Подключаюсь|Определена|Найден Джарвис/], ["Место", /Стираю|Удаляю|Свободно на колонке/], ["Файлы", /Копирую|%$/],
  ["Автозапуск", /Выключаю|Добавляю автозапуск/], ["Запуск панели", /Проверяю запуск панели/], ["Готово", /Готово/]];
function drawStages(lines, done, error, warning) {
  let at = 0, pct = null;
  lines.forEach((l) => { STAGES.forEach(([, re], i) => { if (re.test(l) && i >= at) at = i; }); const m = l.match(/(\d+)%$/); if (m) pct = +m[1]; });
  if (done && !error) at = STAGES.length;
  $("#stages").innerHTML = STAGES.map(([name], i) => {
    const manual = warning && name === "Автозапуск";
    const state = manual ? "bad" : i < at ? "done" : i === at ? (error ? "bad" : "on") : "";
    const detail = manual ? " <small>Вручную</small>" : i === at && i === 2 && pct != null && !done ? ` <small>${pct}%</small>` : "";
    return `<li class="${state}"><i aria-hidden="true"></i>${name}${detail}</li>`;
  }).join("");
  const p = $("#install-progress");
  if (done) p.value = 100; else p.value = Math.round(((at + (at === 2 && pct != null ? pct / 100 : 0.4)) / STAGES.length) * 100);
}
function run(title) {
  step(4);
  $("#stages").hidden = !title.startsWith("Устанавливаю");
  drawStages([], false, false);
  $("#s4-title").textContent = title;
  $("#install-progress").removeAttribute("value");
  $("#install-orb").classList.remove("done", "error");
  $("#job-description").textContent = "Переносим файлы и готовим всё для первого разговора. Пока можно познакомиться с возможностями " + (window.matchMedia("(max-width: 760px)").matches ? "ниже." : "справа.");
  $("#job-status").textContent = "Подключаемся к колонке…";
  $("#job-details").open = false;
  $("#open").classList.add("hidden");
  $("#again").classList.add("hidden");
  const t = setInterval(async () => {
    let d;
    try { d = await api("GET", "/api/job"); } catch { return; }
    $("#log").textContent = (d.lines || []).join("\n");
    $("#log").scrollTop = 1e9;
    $("#job-status").textContent = d.error || d.warning || ((d.lines || []).filter((l) => !/^\s*\d+%$/.test(l)).at(-1)) || "Подготавливаем установку…";
    drawStages(d.lines || [], d.done, d.error, d.warning);
    if (d.done) {
      clearInterval(t);
      $("#s4-title").textContent = d.error ? "Нужна ваша помощь." : d.warning ? "Настройте автозапуск." : "Всё готово.";
      $("#install-progress").value = 100;
      $("#install-orb").classList.add(d.error ? "error" : "done");
      $("#job-description").textContent = d.error ? "Установка остановлена. Причина показана ниже; подробности есть в журнале." : d.warning ? "Джарвис установлен и уже работает." : d.url ? "Джарвис установлен. Откройте панель, подключите любимый AI-сервис и выберите голос." : "Операция завершена. Можно вернуться к выбору колонки.";
      if (d.error) $("#job-details").open = true;
      if (d.url) { $("#open").href = d.url; $("#open").classList.remove("hidden"); }
      $("#again").classList.remove("hidden");
    }
  }, 700);
}
$("#again").addEventListener("click", () => { $("#log").textContent = ""; step(1); });
step(1);

// Product tour is local to the installer and does not contact external services.
const tourSlides = [
  { image: "dashboard", title: "Всё под рукой.", text: "Диалог, музыка и состояние Станции — на одной странице.", alt: "Главная панель YaJavis" },
  { image: "connections", title: "Вы выбираете, с кем говорить.", text: "Подключите OpenAI, Gemini, Grok или ElevenLabs и устройства умного дома.", alt: "Подключения AI и умного дома" },
  { image: "assistant", title: "Голос и характер — ваши.", text: "Выберите голос, модель и характер. Создавайте разных ассистентов и переключайте их в панели.", alt: "Настройки голосового ассистента" },
  { image: "charts", title: "Понимайте свою Станцию.", text: "Память, процессор, температура и Wi-Fi — всё состояние Станции в браузере.", alt: "Графики состояния колонки" },
];
let tourIndex = 0, tourPaused = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
function drawTour() {
  const slide = tourSlides[tourIndex];
  if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    [$("#tour-image"), $(".tour-copy")].forEach(el => {
      el.getAnimations().forEach(a => a.cancel());
      el.animate([{opacity: 0, transform: "translateY(8px)"}, {opacity: 1, transform: "translateY(0)"}], {duration: 420, easing: "cubic-bezier(.22,1,.36,1)"});
    });
  }
  $("#tour-image").src = "screenshots/" + slide.image + ".png";
  $("#tour-image").alt = slide.alt;
  $("#tour-number").textContent = String(tourIndex + 1).padStart(2, "0") + " / 04";
  $("#tour-title").textContent = slide.title;
  $("#tour-text").textContent = slide.text;
  document.querySelectorAll("#tour-dots button").forEach((b, i) => { b.classList.toggle("on", i === tourIndex); b.setAttribute("aria-current", i === tourIndex ? "true" : "false"); });
}
$("#tour-dots").innerHTML = tourSlides.map((s, i) => `<button type="button" data-slide="${i}" aria-label="${esc(s.title)}"></button>`).join("");
$("#tour-dots").addEventListener("click", e => { const b = e.target.closest("[data-slide]"); if (b) { tourPaused = true; drawTourPause(); tourIndex = +b.dataset.slide; drawTour(); } });
$("#tour-next").addEventListener("click", () => { tourPaused = true; drawTourPause(); tourIndex = (tourIndex + 1) % tourSlides.length; drawTour(); });
$("#tour-prev").addEventListener("click", () => { tourPaused = true; drawTourPause(); tourIndex = (tourIndex + tourSlides.length - 1) % tourSlides.length; drawTour(); });
function drawTourPause() {
  $("#tour-pause").setAttribute("aria-pressed", String(tourPaused));
  $("#tour-pause").setAttribute("aria-label", tourPaused ? "Продолжить показ" : "Приостановить показ");
  $("#tour-pause").innerHTML = tourPaused ? '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 5 10 7-10 7Z"/></svg>' : '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 5v14M15 5v14"/></svg>';
}
$("#tour-pause").addEventListener("click", () => { tourPaused = !tourPaused; drawTourPause(); });
setInterval(() => { if (!tourPaused && !document.hidden) { tourIndex = (tourIndex + 1) % tourSlides.length; drawTour(); } }, 8500);
drawTour(); drawTourPause();

// Same cursor-edge treatment used by the panel, limited to one paint per frame.
let pointerFrame = 0, lastPointer = null;
document.addEventListener("pointermove", event => {
  if (event.pointerType === "touch" || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  lastPointer = event;
  if (pointerFrame) return;
  pointerFrame = requestAnimationFrame(() => {
    pointerFrame = 0;
    const card = lastPointer.target.closest(".card");
    if (!card) return;
    const box = card.getBoundingClientRect();
    card.style.setProperty("--mx", (lastPointer.clientX - box.left) + "px");
    card.style.setProperty("--my", (lastPointer.clientY - box.top) + "px");
  });
});
$("#tour-zoom").addEventListener("click", () => {
  tourPaused = true; drawTourPause();
  const slide = tourSlides[tourIndex];
  $("#tour-large").src = $("#tour-image").src;
  $("#tour-large").alt = slide.alt;
  $("#large-title").textContent = slide.title;
  $("#tour-dialog").showModal();
});
$("#tour-close").addEventListener("click", () => $("#tour-dialog").close());
$("#tour-dialog").addEventListener("click", event => { if (event.target === event.currentTarget) event.currentTarget.close(); });
