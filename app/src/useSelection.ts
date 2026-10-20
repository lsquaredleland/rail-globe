import { useMemo, useReducer } from 'react'
import type { Hit } from './MapView'

export type Services = { kind: 'services'; slug: string; ids: number[]; title: string; titleEn: string | null; sub: string }
export type Selected =
  | { kind: 'station'; slug: string; id: string }
  | { kind: 'line'; slug: string; id: number; from?: { sel: Selected; pt: [number, number] | null } }
  | Services
  | { kind: 'lines' }
  | { kind: 'inspect'; hit: Hit }
  | null

/** Everything about what the user has picked or is pointing at. These fields were reset together in many places, so they live in one reducer. */
export interface SelectionState {
  selected: Selected
  selPt: [number, number] | null
  /** the pointer position and the feature under it (desktop tooltip) */
  hover: { hit: Hit; x: number; y: number } | null
  /** the feature a first tap on a touch screen shows, before a second tap opens it */
  peek: Hit | null
  /** the railway name picked on the map or in the Lines list */
  selTrackName: string | null
  /** the region part of the clicked track; null: the Lines list's region */
  selTrackSlug: string | null
  /** the row hovered in a panel */
  listHoverLine: number | null
  listHoverName: string | null
}

const INITIAL: SelectionState = {
  selected: null,
  selPt: null,
  hover: null,
  peek: null,
  selTrackName: null,
  selTrackSlug: null,
  listHoverLine: null,
  listHoverName: null,
}

type Action = { type: 'patch'; p: Partial<SelectionState> } | { type: 'peek'; f: (cur: Hit | null) => Hit | null }

function reducer(s: SelectionState, a: Action): SelectionState {
  const p = a.type === 'peek' ? { peek: a.f(s.peek) } : a.p
  // nothing changes: keep the state object, so a pointer that keeps leaving nothing does not render
  return (Object.keys(p) as (keyof SelectionState)[]).every((k) => s[k] === p[k]) ? s : { ...s, ...p }
}

export function useSelection() {
  const [state, dispatch] = useReducer(reducer, INITIAL)
  // dispatch is stable, so are these: the click handlers that memoize around them stay valid
  const act = useMemo(() => {
    const patch = (p: Partial<SelectionState>) => dispatch({ type: 'patch', p })
    return {
      /** a new region or place was picked: the panel and the tap preview go */
      reset: () => patch({ selected: null, selPt: null, peek: null }),
      openStation: (slug: string, id: string, pt?: [number, number]) =>
        patch({ selected: { kind: 'station', slug, id }, peek: null, hover: null, selTrackName: null, ...(pt ? { selPt: pt } : {}) }),
      setSelPt: (pt: [number, number] | null) => patch({ selPt: pt }),
      openLine: (slug: string, id: number, from?: { sel: Selected; pt: [number, number] | null }) =>
        patch({ selected: { kind: 'line', slug, id, from }, listHoverLine: null, listHoverName: null, peek: null, hover: null, selPt: null }),
      /** back from a line to the station or services it was opened from */
      back: (from: { sel: Selected; pt: [number, number] | null }) => patch({ selected: from.sel, selPt: from.pt, listHoverLine: null }),
      close: () => patch({ selected: null, selPt: null, selTrackName: null, listHoverLine: null, listHoverName: null }),
      openList: () => patch({ selected: { kind: 'lines' }, selPt: null, selTrackName: null, peek: null, hover: null }),
      /** a railway picked in the Lines list: highlight it (the Lines list's region) */
      openNamed: (name: string) => patch({ selTrackName: name, selTrackSlug: null }),
      openInspect: (hit: Hit) =>
        patch({ selected: { kind: 'inspect', hit }, selPt: hit.kind === 'station' ? hit.lngLat : null, peek: null, hover: null, selTrackName: null }),
      openServices: (services: Services) =>
        patch({ selected: services, selPt: null, peek: null, hover: null, selTrackName: null, listHoverLine: null, listHoverName: null }),
      /** a click on a named railway with no line: the Lines list, with that railway highlighted in the region part it was hit in */
      openTrackName: (name: string, slug: string) =>
        patch({ selected: { kind: 'lines' }, selPt: null, peek: null, hover: null, selTrackName: name, selTrackSlug: slug }),
      setHover: (hover: SelectionState['hover']) => patch({ hover }),
      setPeek: (f: (cur: Hit | null) => Hit | null) => dispatch({ type: 'peek', f }),
      clearPeek: () => patch({ peek: null }),
      clearTrackName: () => patch({ selTrackName: null }),
      setListHoverLine: (listHoverLine: number | null) => patch({ listHoverLine }),
      setListHoverName: (listHoverName: string | null) => patch({ listHoverName }),
    }
  }, [])
  return [state, act] as const
}
