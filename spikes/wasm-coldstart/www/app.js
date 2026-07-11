// Vanilla-JS baseline: builds the exact same DOM as the Leptos build, with the same 300 rows,
// the same selection behaviour and the same search filter. The DOM/paint work is identical, so the
// delta between the two runs is purely "what the runtime costs".
const N = 300;

const envelopes = Array.from({ length: N }, (_, i) => ({
  uid: i,
  from: `sender${String(i).padStart(3, '0')}@example.com`,
  subject: `Message ${i} — quarterly summary and next steps`,
  preview: 'Hi, following up on the thread from last week regarding the …',
  date: `${String(i % 24).padStart(2, '0')}:${String(i % 60).padStart(2, '0')}`,
  unread: i % 3 === 0,
}));

let selected = 0;
let query = '';

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}

function renderRead(root) {
  const e = envelopes.find(x => x.uid === selected);
  root.replaceChildren();
  if (!e) return;
  root.append(el('h1', null, e.subject), el('div', 'meta', `${e.from} · ${e.date}`), el('p', null, e.preview));
}

function renderList(listBody, readRoot) {
  const q = query.toLowerCase();
  const visible = envelopes.filter(e => !q || e.subject.toLowerCase().includes(q));
  listBody.replaceChildren(...visible.map(e => {
    const row = el('div', 'row' + (e.unread ? ' unread' : '') + (e.uid === selected ? ' sel' : ''));
    row.append(el('div', 'from', e.from), el('div', 'subj', e.subject),
               el('div', 'prev', e.preview), el('div', 'date', e.date));
    row.onclick = () => { selected = e.uid; renderList(listBody, readRoot); renderRead(readRoot); };
    return row;
  }));
}

export function start() {
  const app = el('div', 'app');

  const rail = el('nav', 'rail');
  rail.append(el('div', 'brand', 'GeleitMail'));
  for (const f of ['Inbox', 'Sent', 'Drafts', 'Archive', 'Spam', 'Saved']) rail.append(el('div', 'folder', f));

  const list = el('section', 'list');
  const search = el('input', 'search');
  search.placeholder = 'Search';
  const listBody = el('div');
  list.append(search, listBody);

  const read = el('article', 'read');
  search.oninput = () => { query = search.value; renderList(listBody, read); };

  renderList(listBody, read);
  renderRead(read);

  app.append(rail, list, read);
  document.body.append(app);
}
