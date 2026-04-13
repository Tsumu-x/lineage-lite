use std::io::Write;

use crate::error::Result;
use crate::graph::node::ColumnLineage;
use crate::graph::LineageGraph;

/// 從完整的 LineageGraph 產生獨立的 HTML 檔案，
/// 內嵌 vanilla JS 繪製互動式 lineage DAG（無外部依賴）。
pub fn render_graph_html(graph: &LineageGraph, col_lineage: &[ColumnLineage], writer: &mut dyn Write) -> Result<()> {
    // 建立 nodes JSON
    let nodes_json: Vec<String> = graph
        .nodes()
        .iter()
        .map(|n| {
            format!(
                r#"{{"id":"{}","kind":"{}"}}"#,
                escape_js(&n.id.0),
                escape_js(&n.kind.to_string())
            )
        })
        .collect();

    // 建立 edges JSON
    let edges_json: Vec<String> = graph
        .edges()
        .iter()
        .map(|e| {
            format!(
                r#"{{"source":"{}","target":"{}","relation":"{}"}}"#,
                escape_js(&e.source.0),
                escape_js(&e.target.0),
                escape_js(&e.relation.to_string())
            )
        })
        .collect();

    // 建立 column lineage JSON
    let col_lineage_json: Vec<String> = col_lineage
        .iter()
        .map(|cl| {
            let sources: Vec<String> = cl.source_columns.iter().map(|s| {
                format!(r#"{{"table":{},"column":"{}"}}"#,
                    s.table.as_ref().map(|t| format!("\"{}\"", escape_js(t))).unwrap_or("null".into()),
                    escape_js(&s.column))
            }).collect();
            format!(
                r#"{{"table":"{}","column":"{}","transform":"{}","sources":[{}],"expr":"{}"}}"#,
                escape_js(&cl.target_table),
                escape_js(&cl.target_column),
                escape_js(&cl.transform.to_string()),
                sources.join(","),
                escape_js(&cl.expression)
            )
        })
        .collect();

    write_html(writer, &nodes_json.join(","), &edges_json.join(","), &col_lineage_json.join(","))?;
    Ok(())
}

/// 輸出完整的 HTML 頁面。
fn write_html(writer: &mut dyn Write, nodes_json: &str, edges_json: &str, col_lineage_json: &str) -> Result<()> {
    write!(writer, r##"<!DOCTYPE html>
<html lang="zh-TW">
<head>
<meta charset="UTF-8">
<title>lineage-lite — Data Lineage Graph</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; background: #0f1117; color: #e0e0e0; overflow: hidden; }}
#header {{ position: fixed; top: 0; left: 0; right: 0; z-index: 100; background: #161922; border-bottom: 1px solid #2a2d3a; padding: 12px 24px; display: flex; align-items: center; justify-content: space-between; }}
#header h1 {{ font-size: 16px; font-weight: 600; color: #fff; }}
#header h1 span {{ color: #6c7ae0; }}
#legend {{ display: flex; gap: 16px; font-size: 12px; }}
.legend-item {{ display: flex; align-items: center; gap: 5px; }}
.legend-dot {{ width: 10px; height: 10px; border-radius: 50%; }}
#info {{ position: fixed; bottom: 20px; left: 20px; z-index: 100; background: #1e2030; border: 1px solid #2a2d3a; border-radius: 8px; padding: 14px 18px; font-size: 13px; max-width: 380px; display: none; box-shadow: 0 4px 20px rgba(0,0,0,0.4); }}
#info h3 {{ color: #6c7ae0; margin-bottom: 6px; font-size: 14px; }}
#info .detail {{ color: #999; margin-top: 4px; line-height: 1.5; }}
#info .detail strong {{ color: #ccc; }}
#stats {{ position: fixed; top: 56px; right: 20px; z-index: 100; background: #1e2030; border: 1px solid #2a2d3a; border-radius: 8px; padding: 10px 14px; font-size: 12px; color: #888; }}
#search {{ position: fixed; top: 56px; left: 20px; z-index: 100; }}
#search input {{ background: #1e2030; border: 1px solid #2a2d3a; border-radius: 6px; padding: 8px 12px; color: #e0e0e0; font-size: 13px; width: 220px; outline: none; }}
#search input:focus {{ border-color: #6c7ae0; }}
svg {{ width: 100vw; height: 100vh; }}
.link {{ fill: none; stroke-opacity: 0.35; }}
.node-label {{ font-size: 11px; fill: #bbb; pointer-events: none; font-weight: 500; }}
.node-circle {{ cursor: pointer; stroke-width: 2; transition: filter 0.15s; }}
.node-circle:hover {{ stroke-width: 3; filter: brightness(1.3) drop-shadow(0 0 6px rgba(108,122,224,0.5)); }}
.node-circle.highlight {{ stroke: #fff !important; stroke-width: 3; filter: brightness(1.4) drop-shadow(0 0 8px rgba(108,122,224,0.7)); }}
.link.highlight {{ stroke-opacity: 0.9; stroke-width: 2.5; }}
.node-circle.dimmed {{ opacity: 0.15; }}
.node-label.dimmed {{ opacity: 0.1; }}
.link.dimmed {{ stroke-opacity: 0.05; }}
</style>
</head>
<body>
<div id="header">
  <h1><span>lineage-lite</span> — Data Lineage Graph</h1>
  <div id="legend">
    <div class="legend-item"><div class="legend-dot" style="background:#6c7ae0"></div>dbt Model</div>
    <div class="legend-item"><div class="legend-dot" style="background:#e0a356"></div>dbt Source</div>
    <div class="legend-item"><div class="legend-dot" style="background:#56b886"></div>SQL Table/View</div>
    <div class="legend-item"><div class="legend-dot" style="background:#e06c8a"></div>Python ETL</div>
  </div>
</div>
<div id="search"><input type="text" placeholder="搜尋節點…" id="searchInput" /></div>
<div id="stats"></div>
<div id="info"></div>
<svg></svg>

<script>
const nodesData = [{nodes_json}];
const edgesData = [{edges_json}];
const colLineage = [{col_lineage_json}];

const kindColor = {{
  'dbt Model': '#6c7ae0',
  'dbt Source': '#e0a356',
  'SQL Table': '#56b886',
  'SQL View':  '#56b886',
  'Python ETL':'#e06c8a',
}};
function getColor(kind) {{ return kindColor[kind] || '#888'; }}

document.getElementById('stats').innerHTML =
  nodesData.length + ' nodes &middot; ' + edgesData.length + ' edges';

const width = window.innerWidth;
const height = window.innerHeight;
const ns = 'http://www.w3.org/2000/svg';
const svg = document.querySelector('svg');
svg.setAttribute('viewBox', '0 0 ' + width + ' ' + height);

// Defs: 箭頭 marker
const defs = document.createElementNS(ns, 'defs');
const marker = document.createElementNS(ns, 'marker');
marker.id = 'arrow';
['viewBox','refX','refY','markerWidth','markerHeight','orient'].forEach((a, i) =>
  marker.setAttribute(a, ['0 0 10 10','22','5','6','6','auto'][i]));
const mp = document.createElementNS(ns, 'path');
mp.setAttribute('d', 'M 0 0 L 10 5 L 0 10 z');
mp.setAttribute('fill', '#555');
marker.appendChild(mp);
defs.appendChild(marker);
svg.appendChild(defs);

const gEl = document.createElementNS(ns, 'g');
svg.appendChild(gEl);

// ===== 拓撲排序分層佈局 =====
const nodeMap = new Map();
nodesData.forEach(n => nodeMap.set(n.id, {{ ...n, x: 0, y: 0, layer: 0 }}));

const children = new Map();
const parents = new Map();
nodeMap.forEach((_, id) => {{ children.set(id, []); parents.set(id, []); }});
edgesData.forEach(e => {{
  if (children.has(e.source)) children.get(e.source).push(e.target);
  if (parents.has(e.target)) parents.get(e.target).push(e.source);
}});

// 用最長路徑決定 layer
function assignLayers() {{
  const inDeg = new Map();
  nodeMap.forEach((_, id) => inDeg.set(id, (parents.get(id) || []).length));
  const queue = [];
  inDeg.forEach((d, id) => {{ if (d === 0) queue.push(id); }});

  while (queue.length > 0) {{
    const id = queue.shift();
    const n = nodeMap.get(id);
    (children.get(id) || []).forEach(cid => {{
      const cn = nodeMap.get(cid);
      if (cn) cn.layer = Math.max(cn.layer, n.layer + 1);
      const newDeg = inDeg.get(cid) - 1;
      inDeg.set(cid, newDeg);
      if (newDeg === 0) queue.push(cid);
    }});
  }}
  // 若有未處理的節點（cycle），給它們 layer 0
  nodeMap.forEach(n => {{ if (n.layer === undefined) n.layer = 0; }});
}}
assignLayers();

const maxLayer = Math.max(...[...nodeMap.values()].map(n => n.layer), 0);
const layerGroups = new Map();
nodeMap.forEach(n => {{
  if (!layerGroups.has(n.layer)) layerGroups.set(n.layer, []);
  layerGroups.get(n.layer).push(n.id);
}});

const xPad = 120;
const yPad = 80;
const layerSpacing = Math.max(Math.min((width - xPad * 2) / (maxLayer + 1), 280), 180);
const nodeSpacing = 56;

layerGroups.forEach((ids, layer) => {{
  const totalH = ids.length * nodeSpacing;
  const startY = Math.max((height - totalH) / 2, yPad);
  ids.forEach((id, i) => {{
    const n = nodeMap.get(id);
    n.x = xPad + layer * layerSpacing;
    n.y = startY + i * nodeSpacing;
  }});
}});

// ===== 繪製邊 =====
const edgeEls = [];
edgesData.forEach((e, idx) => {{
  const s = nodeMap.get(e.source);
  const t = nodeMap.get(e.target);
  if (!s || !t) return;

  const line = document.createElementNS(ns, 'path');
  const mx = (s.x + t.x) / 2;
  line.setAttribute('d', 'M' + s.x + ',' + s.y + ' C' + mx + ',' + s.y + ' ' + mx + ',' + t.y + ' ' + t.x + ',' + t.y);
  line.setAttribute('class', 'link');
  line.setAttribute('stroke', getColor(s.kind));
  line.setAttribute('stroke-width', '1.5');
  line.setAttribute('marker-end', 'url(#arrow)');
  line.dataset.source = e.source;
  line.dataset.target = e.target;
  gEl.appendChild(line);
  edgeEls.push(line);
}});

// ===== 繪製節點 =====
const nodeEls = new Map();
nodeMap.forEach((n) => {{
  const circle = document.createElementNS(ns, 'circle');
  circle.setAttribute('cx', n.x);
  circle.setAttribute('cy', n.y);
  circle.setAttribute('r', '8');
  circle.setAttribute('fill', getColor(n.kind));
  circle.setAttribute('stroke', getColor(n.kind));
  circle.setAttribute('class', 'node-circle');
  circle.dataset.id = n.id;
  gEl.appendChild(circle);

  const label = document.createElementNS(ns, 'text');
  label.setAttribute('x', n.x + 14);
  label.setAttribute('y', n.y + 4);
  label.setAttribute('class', 'node-label');
  label.dataset.id = n.id;
  label.textContent = shortName(n.id);
  gEl.appendChild(label);

  nodeEls.set(n.id, {{ circle, label }});

  circle.addEventListener('click', (ev) => {{
    ev.stopPropagation();
    highlightNode(n.id);
    showInfo(n);
  }});
}});

function shortName(id) {{
  const parts = id.split('/');
  return parts[parts.length - 1];
}}

// ===== 高亮連鎖 =====
function highlightNode(id) {{
  const related = new Set([id]);
  edgesData.forEach(e => {{
    if (e.source === id) related.add(e.target);
    if (e.target === id) related.add(e.source);
  }});

  nodeEls.forEach((els, nid) => {{
    if (related.has(nid)) {{
      els.circle.classList.remove('dimmed');
      els.circle.classList.toggle('highlight', nid === id);
      els.label.classList.remove('dimmed');
    }} else {{
      els.circle.classList.add('dimmed');
      els.circle.classList.remove('highlight');
      els.label.classList.add('dimmed');
    }}
  }});
  edgeEls.forEach(el => {{
    if (el.dataset.source === id || el.dataset.target === id) {{
      el.classList.add('highlight');
      el.classList.remove('dimmed');
    }} else {{
      el.classList.add('dimmed');
      el.classList.remove('highlight');
    }}
  }});
}}

function clearHighlight() {{
  nodeEls.forEach(els => {{
    els.circle.classList.remove('dimmed', 'highlight');
    els.label.classList.remove('dimmed');
  }});
  edgeEls.forEach(el => {{
    el.classList.remove('dimmed', 'highlight');
  }});
  document.getElementById('info').style.display = 'none';
}}

// ===== 資訊面板 =====
function showInfo(n) {{
  const info = document.getElementById('info');
  const ups = edgesData.filter(e => e.target === n.id);
  const downs = edgesData.filter(e => e.source === n.id);
  const cols = colLineage.filter(c => c.table === n.id);

  let html =
    '<h3>' + shortName(n.id) + '</h3>' +
    '<div class="detail"><strong>ID:</strong> ' + n.id + '</div>' +
    '<div class="detail"><strong>類型:</strong> ' + n.kind + '</div>' +
    '<div class="detail"><strong>上游 (' + ups.length + '):</strong> ' +
      (ups.map(e => shortName(e.source)).join(', ') || '無') + '</div>' +
    '<div class="detail"><strong>下游 (' + downs.length + '):</strong> ' +
      (downs.map(e => shortName(e.target)).join(', ') || '無') + '</div>';

  if (cols.length > 0) {{
    html += '<div class="detail" style="margin-top:8px"><strong>欄位 Lineage:</strong></div>';
    html += '<table style="width:100%;font-size:11px;margin-top:4px;border-collapse:collapse">';
    html += '<tr style="color:#6c7ae0;border-bottom:1px solid #2a2d3a"><td>欄位</td><td>轉換</td><td>來源</td></tr>';
    cols.forEach(c => {{
      const src = c.sources.map(s => s.table ? s.table.split('.').pop() + '.' + s.column : s.column).join(', ') || '—';
      const trColor = c.transform.includes('()') ? '#e0a356' : (c.transform === 'expression' ? '#e06c8a' : '#888');
      html += '<tr style="border-bottom:1px solid #1a1c28">' +
        '<td style="padding:2px 4px;color:#ccc">' + c.column + '</td>' +
        '<td style="padding:2px 4px;color:' + trColor + '">' + c.transform + '</td>' +
        '<td style="padding:2px 4px;color:#888">' + src + '</td></tr>';
    }});
    html += '</table>';
  }}

  info.style.display = 'block';
  info.innerHTML = html;
}}

svg.addEventListener('click', (e) => {{
  if (e.target === svg || e.target === gEl) clearHighlight();
}});

// ===== 搜尋 =====
document.getElementById('searchInput').addEventListener('input', function() {{
  const q = this.value.toLowerCase();
  if (!q) {{ clearHighlight(); return; }}
  const match = [...nodeMap.keys()].find(id => id.toLowerCase().includes(q));
  if (match) {{
    highlightNode(match);
    showInfo(nodeMap.get(match));
  }}
}});

// ===== Zoom & Pan =====
let scale = 1, tx = 0, ty = 0;
function updateTransform() {{
  gEl.setAttribute('transform', 'translate(' + tx + ',' + ty + ') scale(' + scale + ')');
}}
svg.addEventListener('wheel', (e) => {{
  e.preventDefault();
  const rect = svg.getBoundingClientRect();
  const mx = e.clientX - rect.left;
  const my = e.clientY - rect.top;
  const factor = e.deltaY > 0 ? 0.92 : 1.08;
  const newScale = Math.max(0.1, Math.min(5, scale * factor));
  tx = mx - (mx - tx) * (newScale / scale);
  ty = my - (my - ty) * (newScale / scale);
  scale = newScale;
  updateTransform();
}});
let dragging = false, lastX, lastY;
svg.addEventListener('mousedown', (e) => {{
  if (e.target.classList.contains('node-circle')) return;
  dragging = true; lastX = e.clientX; lastY = e.clientY;
  svg.style.cursor = 'grabbing';
}});
svg.addEventListener('mousemove', (e) => {{
  if (!dragging) return;
  tx += e.clientX - lastX; ty += e.clientY - lastY;
  lastX = e.clientX; lastY = e.clientY;
  updateTransform();
}});
svg.addEventListener('mouseup', () => {{ dragging = false; svg.style.cursor = 'default'; }});
svg.addEventListener('mouseleave', () => {{ dragging = false; }});

// 初始 fit-to-view
const allX = [...nodeMap.values()].map(n => n.x);
const allY = [...nodeMap.values()].map(n => n.y);
if (allX.length > 0) {{
  const minX = Math.min(...allX) - 60, maxX = Math.max(...allX) + 180;
  const minY = Math.min(...allY) - 40, maxY = Math.max(...allY) + 40;
  const gW = maxX - minX, gH = maxY - minY;
  scale = Math.min(width / gW, (height - 80) / gH, 1.5) * 0.85;
  tx = (width - gW * scale) / 2 - minX * scale;
  ty = (height - gH * scale) / 2 - minY * scale + 30;
  updateTransform();
}}
</script>
</body>
</html>"##)?;
    Ok(())
}

fn escape_js(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
