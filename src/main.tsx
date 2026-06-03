import { Component, lazy, StrictMode, Suspense, type ReactNode } from 'react'
import * as Sentry from '@sentry/react'
import { createRoot } from 'react-dom/client'
import { Button } from '@/components/ui/button'
import { TooltipProvider } from '@/components/ui/tooltip'
import './index.css'
import { FrontendReadyMarker } from './components/FrontendReadyMarker'
import { LinuxTitlebar } from './components/LinuxTitlebar'
import { applyStoredThemeMode } from './lib/themeMode'
import {
  APP_COMMAND_EVENT_NAME,
  isAppCommandId,
  isNativeMenuCommandId,
} from './hooks/appCommandDispatcher'
import {
  getShortcutEventInit,
  type AppCommandShortcutEventInit,
  type AppCommandShortcutEventOptions,
} from './hooks/appCommandCatalog'
import { isRecoveredBlockNoteRenderError } from './components/blockNoteRenderRecovery'
import { DEFAULT_APP_LOCALE, translate } from './lib/i18n'
import { isMac, shouldUseCustomWindowChrome } from './utils/platform'
import { reloadFrontendOnceIfStartupFailed } from './utils/frontendReady'

const TLDRAW_CONTEXT_MENU_SELECTOR = '.tldraw-whiteboard'

const RootApp = lazy(() => import('./App.tsx'))

type RootErrorBoundaryState = {
  error: Error | null
}

class RootErrorBoundary extends Component<{ children: ReactNode }, RootErrorBoundaryState> {
  state: RootErrorBoundaryState = { error: null }

  static getDerivedStateFromError(error: Error): RootErrorBoundaryState {
    return { error }
  }

  componentDidCatch(): void {
    // React root callbacks own Sentry reporting; this boundary owns the visible fallback.
  }

  render(): ReactNode {
    if (this.state.error === null) return this.props.children

    return (
      <section
        id="tolaria-root-error-boundary"
        role="alert"
        style={{
          alignItems: 'center',
          background: 'Canvas',
          color: 'CanvasText',
          display: 'flex',
          flexDirection: 'column',
          gap: 12,
          minHeight: '100vh',
          padding: 24,
          textAlign: 'center',
        }}
      >
        <h1 style={{ fontSize: 20, margin: 0 }}>{translate(DEFAULT_APP_LOCALE, 'rootError.openTitle')}</h1>
        <p style={{ margin: 0, maxWidth: 520 }}>
          {translate(DEFAULT_APP_LOCALE, 'rootError.openDescription')}
        </p>
        <Button type="button" onClick={() => window.location.reload()}>
          {translate(DEFAULT_APP_LOCALE, 'rootError.reloadAction')}
        </Button>
      </section>
    )
  }
}

function dataTransferHasFiles(dataTransfer: DataTransfer | null): boolean {
  if (!dataTransfer) return false
  if (dataTransfer.files.length > 0) return true
  if (Array.from(dataTransfer.types).includes('Files')) return true

  return Array.from(dataTransfer.items).some((item) => item.kind === 'file')
}

function preventFileDropNavigation(event: DragEvent): void {
  if (!dataTransferHasFiles(event.dataTransfer)) return

  event.preventDefault()
}

function isTldrawContextMenuTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(TLDRAW_CONTEXT_MENU_SELECTOR) !== null
}

function preventNativeContextMenu(event: MouseEvent): void {
  if (isTldrawContextMenuTarget(event.target)) return

  event.preventDefault()
}

document.addEventListener('dragover', preventFileDropNavigation, true)
document.addEventListener('drop', preventFileDropNavigation, true)

// Disable native WebKit context menu in Tauri (WKWebView intercepts right-click
// at native level before React's synthetic events can call preventDefault).
// Capture phase fires first → prevents native menu; React bubble phase still fires
// → our custom context menus (e.g. sidebar right-click) work correctly.
if ('__TAURI__' in window || '__TAURI_INTERNALS__' in window) {
  document.addEventListener('contextmenu', preventNativeContextMenu, true)
}

if (shouldUseCustomWindowChrome()) {
  document.body.classList.add('custom-window-chrome')
}

if (isMac()) {
  document.body.classList.add('mac-chrome')
}

applyStoredThemeMode(document, window.localStorage)

function dispatchDeterministicShortcutEvent(init: AppCommandShortcutEventInit) {
  const target =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : document.body ?? window

  target.dispatchEvent(new KeyboardEvent('keydown', init))
}

window.__laputaTest = {
  dispatchAppCommand(id: string) {
    if (!isAppCommandId(id)) {
      throw new Error(`Unknown app command: ${id}`)
    }
    window.dispatchEvent(new CustomEvent(APP_COMMAND_EVENT_NAME, { detail: id }))
  },
  dispatchShortcutEvent(init: AppCommandShortcutEventInit) {
    dispatchDeterministicShortcutEvent(init)
  },
  async triggerMenuCommand(id: string) {
    if (!isNativeMenuCommandId(id)) {
      throw new Error(`Unknown native menu command: ${id}`)
    }

    if ('__TAURI__' in window || '__TAURI_INTERNALS__' in window) {
      const { invoke } = await import('@tauri-apps/api/core')
      return invoke('trigger_menu_command', { id })
    }

    if (!window.__laputaTest?.dispatchBrowserMenuCommand) {
      throw new Error('Tolaria test bridge is missing dispatchBrowserMenuCommand')
    }

    window.__laputaTest.dispatchBrowserMenuCommand(id)
    return undefined
  },
  triggerShortcutCommand(id: string, options?: AppCommandShortcutEventOptions) {
    if (!isAppCommandId(id)) {
      throw new Error(`Unknown app command: ${id}`)
    }

    const init = getShortcutEventInit(id, options)
    if (!init) {
      throw new Error(`Command ${id} does not define a keyboard shortcut`)
    }

    dispatchDeterministicShortcutEvent(init)
  },
}

const sentryReactErrorHandler = Sentry.reactErrorHandler()

function isResizeObserverLoopError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error)
  return message.includes('ResizeObserver loop completed with undelivered notifications')
    || message.includes('ResizeObserver loop limit exceeded')
}

function showFatalRenderError(
  error: unknown,
  errorInfo: { componentStack?: string },
): void {
  const existing = document.getElementById('tolaria-fatal-render-error')
  const overlay = existing ?? document.createElement('section')
  overlay.id = 'tolaria-fatal-render-error'
  overlay.setAttribute('role', 'alert')
  overlay.style.cssText = [
    'position:fixed',
    'inset:24px',
    'z-index:2147483647',
    'overflow:auto',
    'margin:0',
    'padding:16px',
    'border-radius:8px',
    'background:#1f1f1f',
    'color:#fff',
    'font:14px/1.5 system-ui,sans-serif',
  ].join(';')

  const message = error instanceof Error ? error.stack ?? error.message : String(error)
  overlay.innerHTML = ''
  const title = document.createElement('h1')
  title.textContent = translate(DEFAULT_APP_LOCALE, 'rootError.renderTitle')
  const body = document.createElement('p')
  body.textContent = translate(DEFAULT_APP_LOCALE, 'rootError.renderDescription')
  const details = document.createElement('details')
  const summary = document.createElement('summary')
  summary.textContent = translate(DEFAULT_APP_LOCALE, 'rootError.technicalDetails')
  const pre = document.createElement('pre')
  pre.style.whiteSpace = 'pre-wrap'
  pre.textContent = [message, errorInfo.componentStack ?? ''].join('\n\n')
  details.append(summary, pre)
  overlay.append(title, body, details)
  document.body.appendChild(overlay)
}

function reportReactRootError(
  error: unknown,
  errorInfo: { componentStack?: string },
): void {
  const componentStack = errorInfo.componentStack ?? ''
  sentryReactErrorHandler(error, { componentStack })
  reloadFrontendOnceIfStartupFailed()
}

function captureReactRootError(
  error: unknown,
  errorInfo: { componentStack?: string },
): void {
  if (isResizeObserverLoopError(error)) return

  const componentStack = errorInfo.componentStack ?? ''
  showFatalRenderError(error, { componentStack })
  reportReactRootError(error, { componentStack })
}

function captureRecoverableReactRootError(
  error: unknown,
  errorInfo: { componentStack?: string },
): void {
  const componentStack = errorInfo.componentStack ?? ''
  if (isResizeObserverLoopError(error)) return
  if (isRecoveredBlockNoteRenderError(error, componentStack)) return

  reportReactRootError(error, { componentStack })
}

createRoot(document.getElementById('root')!, {
  onCaughtError: captureRecoverableReactRootError,
  onUncaughtError: captureReactRootError,
  onRecoverableError: captureRecoverableReactRootError,
}).render(
  <StrictMode>
    <TooltipProvider>
      <LinuxTitlebar />
      <Suspense fallback={null}>
        <RootErrorBoundary>
          <RootApp />
          <FrontendReadyMarker />
        </RootErrorBoundary>
      </Suspense>
    </TooltipProvider>
  </StrictMode>,
)
