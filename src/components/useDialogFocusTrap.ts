import { useEffect, type RefObject } from 'react'

const isFocusable = (element: HTMLElement): boolean => {
  if (element.tabIndex < 0) return false
  if (element.getAttribute('aria-disabled') === 'true') return false
  if (element.getAttribute('disabled') !== null) return false
  return true
}

const focusableElementsIn = (dialog: HTMLElement): HTMLElement[] => {
  return Array.from(dialog.querySelectorAll<HTMLElement>('*')).filter(isFocusable)
}

function focusBoundaryTarget(dialog: HTMLElement, shiftKey: boolean): HTMLElement | null {
  const focusableElements = focusableElementsIn(dialog)
  if (focusableElements.length === 0) return dialog

  const activeElement = document.activeElement
  const firstElement = focusableElements[0]
  const lastElement = focusableElements.at(-1) ?? firstElement
  if (!(activeElement instanceof Element) || !dialog.contains(activeElement)) {
    return shiftKey ? lastElement : firstElement
  }
  if (shiftKey && activeElement === firstElement) return lastElement
  if (!shiftKey && activeElement === lastElement) return firstElement
  return null
}

export function useDialogFocusTrap(dialogRef: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return

      const dialog = dialogRef.current
      if (!dialog) return

      const target = focusBoundaryTarget(dialog, event.shiftKey)
      if (!target) return

      event.preventDefault()
      target.focus()
    }

    document.addEventListener('keydown', handleKeyDown, true)
    return () => document.removeEventListener('keydown', handleKeyDown, true)
  }, [dialogRef])
}
