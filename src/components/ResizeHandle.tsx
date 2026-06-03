import { useCallback, useEffect, useRef, type KeyboardEvent } from 'react'

interface ResizeHandleProps {
  ariaLabel?: string
  onResize: (delta: number) => void
}

const KEYBOARD_RESIZE_STEP = 24

export function ResizeHandle({ ariaLabel = 'Resize pane', onResize }: ResizeHandleProps) {
  const handleRef = useRef<HTMLDivElement>(null)
  const isDragging = useRef(false)
  const lastX = useRef(0)
  const pendingDelta = useRef(0)
  const rafId = useRef(0)

  const handleMouseDown = useCallback(
    (e: MouseEvent) => {
      e.preventDefault()
      isDragging.current = true
      lastX.current = e.clientX
      pendingDelta.current = 0
      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'
    },
    [],
  )

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return
      pendingDelta.current += e.clientX - lastX.current
      lastX.current = e.clientX

      if (!rafId.current) {
        rafId.current = requestAnimationFrame(() => {
          if (pendingDelta.current !== 0) {
            onResize(pendingDelta.current)
            pendingDelta.current = 0
          }
          rafId.current = 0
        })
      }
    }

    const handleMouseUp = () => {
      if (isDragging.current) {
        isDragging.current = false
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
        // Flush any pending delta
        if (rafId.current) {
          cancelAnimationFrame(rafId.current)
          rafId.current = 0
        }
        if (pendingDelta.current !== 0) {
          onResize(pendingDelta.current)
          pendingDelta.current = 0
        }
      }
    }

    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)
    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
      if (rafId.current) cancelAnimationFrame(rafId.current)
    }
  }, [onResize])

  useEffect(() => {
    const handle = handleRef.current
    if (!handle) return
    handle.addEventListener('mousedown', handleMouseDown)
    return () => handle.removeEventListener('mousedown', handleMouseDown)
  }, [handleMouseDown])

  const handleKeyDown = useCallback((event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return

    event.preventDefault()
    onResize(event.key === 'ArrowLeft' ? -KEYBOARD_RESIZE_STEP : KEYBOARD_RESIZE_STEP)
  }, [onResize])

  return (
    <div
      ref={handleRef}
      role="separator"
      aria-label={ariaLabel}
      aria-orientation="vertical"
      tabIndex={0}
      className="relative z-30 -ml-1 w-1 shrink-0 self-stretch cursor-col-resize bg-transparent transition-colors hover:bg-[var(--border)] focus-visible:bg-[var(--border)] focus-visible:outline-none"
      onKeyDown={handleKeyDown}
    />
  )
}
