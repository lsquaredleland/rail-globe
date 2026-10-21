import { useEffect, useState } from 'react'
import type { Hit } from '../MapView'
import { loadFeatureTags, loadLine, loadStation } from '../data'
import { ignoreError } from '../dev'
import { Icon } from './ui'
import { PanelFrame } from './Panels'

interface Inspected {
  title: string
  osm: string | null
  tags: [string, string][]
  derived: [string, string][]
  sources?: string[]
}

const DERIVED_KEYS = new Set(['type', 'type_source', 'own_type', 'construction_type'])
const isDerived = (k: string) => k.startsWith('_') || DERIVED_KEYS.has(k)
const fmt = (v: unknown): string => (Array.isArray(v) ? v.join(';') : v !== null && typeof v === 'object' ? JSON.stringify(v) : String(v))
const pairs = (o: Record<string, unknown>) =>
  Object.entries(o)
    .filter(([, v]) => v !== null && v !== undefined && v !== '')
    .map(([k, v]) => [k, fmt(v)] as [string, string])

async function inspect(hit: Hit): Promise<Inspected | null> {
  const id = String(hit.props.id ?? '')
  if (hit.kind === 'station') {
    const s = await loadStation(hit.slug, id)
    const feat = s.tags.find((t) => /^(node|way)\//.test(t.id)) ?? s.tags[0]
    const all = Object.entries(feat?.tags ?? {}).filter(([k]) => !isDerived(k))
    return {
      title: feat?.id ?? s.id,
      osm: s.osm,
      tags: pairs(Object.fromEntries(all)),
      derived: [...pairs(Object.fromEntries(Object.entries(feat?.tags ?? {}).filter(([k]) => isDerived(k)))), ...pairs(s.model)],
      sources: s.tags.map((t) => t.id),
    }
  }
  if (hit.kind === 'line') {
    const l = await loadLine(hit.slug, hit.props.lid as number)
    return {
      title: l.osm,
      osm: l.osm,
      tags: pairs(l.tags),
      derived: pairs({ type: l.type, 'route relations': l.route_ids.length, 'length (longest route, km)': l.km }),
      sources: l.route_ids.slice(0, 6),
    }
  }
  // zoomed out, track comes from the merged overview: no OSM id to look up
  if (!id) return { title: 'Zoom in (z9 or closer) to inspect this track', osm: null, tags: [], derived: [] }
  const t = await loadFeatureTags(hit.slug, id)
  if (!t) return { title: id, osm: id || null, tags: [], derived: [] }
  return {
    title: id,
    osm: id,
    tags: pairs(Object.fromEntries(Object.entries(t).filter(([k]) => !isDerived(k)))),
    derived: pairs(Object.fromEntries(Object.entries(t).filter(([k]) => isDerived(k)))),
  }
}

function useInspected(hit: Hit): Inspected | null | undefined {
  const key = `${hit.kind}:${hit.slug}:${hit.props.id ?? hit.props.lid}`
  const [s, setS] = useState<{ key: string; v: Inspected | null } | null>(null)
  useEffect(() => {
    let live = true
    inspect(hit)
      .then((v) => live && setS({ key, v }))
      .catch(() => live && setS({ key, v: null }))
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return s && s.key === key ? s.v : undefined
}

const osmUrl = (id: string | null) => (id && /^(node|way|relation)\//.test(id) ? `https://www.openstreetmap.org/${id}` : null)

function Kv({ rows, limit }: { rows: [string, string][]; limit?: number }) {
  const shown = limit ? rows.slice(0, limit) : rows
  return (
    <dl className="kv">
      {shown.map(([k, v]) => (
        <div key={k}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
      {limit && rows.length > limit && <div className="more-tags">+ {rows.length - limit} more · click to pin</div>}
    </dl>
  )
}

export function InspectTooltipBody({ hit }: { hit: Hit }) {
  const d = useInspected(hit)
  if (d === undefined) return <div className="muted">Loading tags…</div>
  if (d === null) return <div className="muted">No tags found for this object.</div>
  return (
    <>
      <div className="insp-head">
        <span className="mono">{d.title}</span>
        <span className="muted sm">{d.tags.length} tags</span>
      </div>
      <Kv rows={d.tags} limit={14} />
    </>
  )
}

export function InspectPanel({ hit, onClose }: { hit: Hit; onClose(): void }) {
  const d = useInspected(hit)
  const [copied, setCopied] = useState(false)
  if (d === undefined)
    return (
      <PanelFrame onClose={onClose} label="Inspect">
        <p className="muted">Loading tags…</p>
      </PanelFrame>
    )
  if (d === null)
    return (
      <PanelFrame onClose={onClose} label="Inspect">
        <p className="muted">No tags found for this object.</p>
      </PanelFrame>
    )
  const link = osmUrl(d.osm)
  const copy = () => {
    navigator.clipboard?.writeText(d.tags.map(([k, v]) => `${k}=${v}`).join('\n')).catch(ignoreError('clipboard'))
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }
  return (
    <PanelFrame onClose={onClose} label="Inspect">
      <div className="eyebrow">Inspect · {hit.kind}</div>
      <h2 className="p-title mono">{d.title}</h2>
      <div className="p-sub">{d.tags.length} OSM tags kept by the Rail Globe filter</div>
      <div className="row gap8">
        {link && (
          <a className="btn outline-btn" href={link} target="_blank" rel="noopener noreferrer">
            Open in OSM ↗
          </a>
        )}
        <button type="button" className="btn outline-btn" onClick={copy}>
          {copied ? Icon.check : null}
          {copied ? 'Copied' : 'Copy tags'}
        </button>
      </div>
      <Kv rows={d.tags} />
      {d.derived.length > 0 && (
        <section>
          <h3 className="eyebrow">Derived by Rail Globe</h3>
          <Kv rows={d.derived} />
        </section>
      )}
      {d.sources && d.sources.length > 1 && (
        <p className="muted sm">
          Built from {d.sources.length} OSM objects: {d.sources.join(', ')}
        </p>
      )}
      <p className="muted sm">Only the tags in the data spec are kept; everything else in OSM was dropped at filter time.</p>
    </PanelFrame>
  )
}
