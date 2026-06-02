import { useCallback, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

import { trackEvent } from '../lib/telemetry'

/** One note that could not be imported, mirroring the Rust `SkippedNote`. */
export interface AppleNotesSkippedNote {
  source_id: string
  reason: string
}

/** Result of an import run, mirroring the Rust `ImportReport` (serde field names). */
export interface AppleNotesImportReport {
  imported: number
  updated: number
  preserved: string[]
  skipped: AppleNotesSkippedNote[]
}

export type AppleNotesImportStatus = 'idle' | 'running' | 'done' | 'error'

interface AppleNotesImportState {
  status: AppleNotesImportStatus
  report: AppleNotesImportReport | null
  error: string | null
  /** True when the failure was a missing Full Disk Access grant. */
  fdaRequired: boolean
}

const FDA_MARKER = 'FDA_REQUIRED:'

const IDLE_STATE: AppleNotesImportState = {
  status: 'idle',
  report: null,
  error: null,
  fdaRequired: false,
}

/**
 * Drive an Apple Notes import against `vaultPath`. The Rust command does the
 * work; this tracks UI state and reports a completion/failure event with safe,
 * content-free metadata.
 */
export function useAppleNotesImport(vaultPath: string) {
  const [state, setState] = useState<AppleNotesImportState>(IDLE_STATE)

  const runImport = useCallback(async () => {
    setState({ status: 'running', report: null, error: null, fdaRequired: false })
    try {
      const report = await invoke<AppleNotesImportReport>('import_apple_notes', { vaultPath })
      setState({ status: 'done', report, error: null, fdaRequired: false })
      trackEvent('apple_notes_import_completed', {
        imported: report.imported,
        updated: report.updated,
        preserved: report.preserved.length,
        skipped: report.skipped.length,
      })
    } catch (caught) {
      const message = typeof caught === 'string' ? caught : String(caught)
      const fdaRequired = message.startsWith(FDA_MARKER)
      setState({ status: 'error', report: null, error: message, fdaRequired })
      trackEvent('apple_notes_import_failed', { fda_required: fdaRequired ? 1 : 0 })
    }
  }, [vaultPath])

  const reset = useCallback(() => setState(IDLE_STATE), [])

  return { ...state, runImport, reset }
}
