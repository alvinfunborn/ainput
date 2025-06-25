import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

function App() {
  // Overlay 相关状态
  const [overlayVisible, setOverlayVisible] = useState(false)
  const [candidate, setCandidate] = useState('')
  const candidateDivRef = useRef<HTMLDivElement>(null)
  const candidateRef = useRef('')
  const [cssText, setCssText] = useState('')

  // 监听 Tauri 事件
  useEffect(() => {
    console.info('[App] useEffect: Setting up event listeners')
    let unlistenUpdate: (() => void) | null = null
    let unlistenHide: (() => void) | null = null
    let unlistenSelectCandidate: (() => void) | null = null
    let cancelled = false

    import('@tauri-apps/api/event').then(({ listen }) => {
      console.info('[App] Event module loaded, registering listeners...')
      listen('update_overlay', (event: any) => {
        setOverlayVisible(true)
        const newChar = Array.isArray(event.payload) ? event.payload[0] : event.payload
        candidateRef.current += newChar || ''
        setCandidate(candidateRef.current)
        console.debug(`[App] event: update_overlay, new char: ${newChar}, new candidate length: ${candidateRef.current.length}`)
      }).then((unlisten) => {
        if (cancelled) unlisten()
        else unlistenUpdate = unlisten
      })
      listen('hide_overlay', () => {
        console.info('[App] event: hide_overlay')
        setOverlayVisible(false)
        setCandidate('')
      }).then((unlisten) => {
        if (cancelled) unlisten()
        else unlistenHide = unlisten
      })
      listen('select_candidate', (event: any) => {
        console.info(`[App] event: select_candidate, payload: ${event.payload}`)
        // 取 candidate 的前 n 位，-1 表示全部
        let n = typeof event.payload === 'number' ? event.payload : 1;
        const chars = Array.from(candidateRef.current);
        if (n === -1) n = chars.length;
        // candidate 去掉前 n 位
        const rest = chars.slice(n).join('');
        candidateRef.current = rest;
        setCandidate(candidateRef.current);
        console.info(`[App] event: select_candidate, rest: ${rest}`)
        if (rest.length === 0) {
          setOverlayVisible(false);
        }
      }).then((unlisten) => {
        if (cancelled) unlisten()
        else unlistenSelectCandidate = unlisten
      })
    })
    // 去掉主窗口滚动条
    document.body.style.overflow = 'hidden';
    return () => {
      console.info('[App] useEffect cleanup: Removing event listeners')
      cancelled = true
      unlistenUpdate && unlistenUpdate()
      unlistenHide && unlistenHide()
      unlistenSelectCandidate && unlistenSelectCandidate()
      document.body.style.overflow = '';
    }
  }, [])

  useEffect(() => {
    console.info('[App] useEffect: overlay visible, fetching style')
    invoke('get_overlay_style').then((style: any) => {
      console.info('[App] useEffect: got overlay style')
      if (typeof style === 'string') {
        setCssText(`.hint ${style} `)
      }
      })
  }, [])

  useEffect(() => {
    if (overlayVisible && candidate) {
      setTimeout(resizeToFitContent, 0)
    }
  }, [candidate, overlayVisible])

  useEffect(() => {
    candidateRef.current = candidate
  }, [candidate])

  function resizeToFitContent() {
    const div = candidateDivRef.current
    if (div) {
      requestAnimationFrame(() => {
        const rect = div.getBoundingClientRect()
        const width = Math.ceil(rect.width)
        const height = Math.ceil(rect.height)
        console.info(`[App] resizing window to fit content: width=${width}, height=${height}`)
        invoke('resize_overlay_window', { width, height })
      })
    }
  }

  return (
    <>
      <style>{cssText}</style>
      {overlayVisible && (
        <div
          id="container"
          style={{
            position: 'absolute',
            left: 0,
            top: 0,
            width: '100vw',
            height: '100vh',
            borderRadius: 0,
            boxShadow: 'none',
            border: 'none',
            outline: 'none',
            display: 'flex',
            backgroundColor: 'transparent',
            flexDirection: 'column',
            justifyContent: 'center',
            alignItems: 'center',
            whiteSpace: 'nowrap',
          }}
        >
          <div id="candidate" ref={candidateDivRef} className="hint">{candidate}</div>
        </div>
      )}
    </>
  )
}

export default App
