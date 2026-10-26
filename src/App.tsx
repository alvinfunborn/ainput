import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import './Overlay.css'

function App() {
  const [overlayVisible, setOverlayVisible] = useState(false)
  const [candidate, setCandidate] = useState('')
  const [statusMessage, setStatusMessage] = useState('')
  const [limits, setLimits] = useState<[number, number]>([560, 32])
  const candidateDivRef = useRef<HTMLDivElement>(null)
  const candidateRef = useRef('')
  const [cssText, setCssText] = useState('')

  useEffect(() => {
    const unlisteners: (() => void)[] = []
    let cancelled = false
    const registered = (unlisten: () => void) => {
      if (cancelled) unlisten()
      else unlisteners.push(unlisten)
    }
    import('@tauri-apps/api/event').then(({ listen }) => {
      listen<string | string[]>('update_overlay', (event) => {
        setStatusMessage('')
        setOverlayVisible(true)
        const chunk = Array.isArray(event.payload) ? event.payload[0] : event.payload
        candidateRef.current += chunk || ''
        setCandidate(candidateRef.current)
      }).then(registered)
      listen<string>('replace_overlay', ({ payload }) => {
        candidateRef.current = payload
        setCandidate(payload)
        setStatusMessage('')
        setOverlayVisible(!!payload)
      }).then(registered)
      listen('hide_overlay', () => {
        setOverlayVisible(false)
        candidateRef.current = ''
        setCandidate('')
        setStatusMessage('')
      }).then(registered)
      listen<string>('insertion_error', (event) => {
        setStatusMessage(event.payload)
      }).then(registered)
      listen<string>('request_error', (event) => {
        candidateRef.current = ''
        setCandidate('')
        setStatusMessage(event.payload)
        setOverlayVisible(true)
      }).then(registered)
      listen<[number, number]>('overlay_limits', ({ payload }) => {
        if (payload.every(value => Number.isFinite(value) && value > 0)) {
          setLimits(previous => previous[0] === payload[0] && previous[1] === payload[1] ? previous : payload)
        }
      }).then(registered)
      listen<number>('select_candidate', ({ payload }) => {
        const chars = Array.from(candidateRef.current)
        const n = payload === -1 ? chars.length : payload
        candidateRef.current = chars.slice(n).join('')
        setCandidate(candidateRef.current)
        if (!candidateRef.current) setOverlayVisible(false)
      }).then(registered)
    })
    document.body.style.overflow = 'hidden'
    return () => {
      cancelled = true
      unlisteners.forEach(unlisten => unlisten())
      document.body.style.overflow = ''
    }
  }, [])

  useEffect(() => {
    invoke<string>('get_overlay_style').then(style => {
      if (typeof style === 'string') setCssText(`.hint ${style}`)
    })
  }, [])

  useEffect(() => {
    const wrapper = candidateDivRef.current
    if (!overlayVisible || !wrapper) return
    let frame = 0
    const measure = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        const rect = wrapper.getBoundingClientRect()
        if (rect.width > 0 && rect.height > 0) {
          void invoke('resize_overlay_window', {
            width: Math.ceil(rect.width), height: Math.ceil(rect.height),
          })
        }
      })
    }
    const observer = new ResizeObserver(measure)
    observer.observe(wrapper)
    measure()
    return () => {
      observer.disconnect()
      cancelAnimationFrame(frame)
    }
  }, [candidate, overlayVisible, statusMessage, limits, cssText])

  return (
    <>
      <style>{cssText}</style>
      {overlayVisible && (
        <div
          id="container"
          ref={candidateDivRef}
          className="overlay-content"
          style={{ maxWidth: limits[0], maxHeight: limits[1] }}
        >
          {candidate && !statusMessage && <div id="candidate" className="hint">
            {candidate.replace(/\r\n|\r|\n/g, ' ↵ ')}
          </div>}
          {statusMessage && <div role="status" className="overlay-status">{statusMessage}</div>}
        </div>
      )}
    </>
  )
}

export default App
