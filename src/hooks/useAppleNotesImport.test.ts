import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'

import { useAppleNotesImport, type AppleNotesImportReport } from './useAppleNotesImport'
import { invoke } from '@tauri-apps/api/core'
import { trackEvent } from '../lib/telemetry'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('../lib/telemetry', () => ({ trackEvent: vi.fn() }))

const invokeMock = vi.mocked(invoke)
const trackEventMock = vi.mocked(trackEvent)

const REPORT: AppleNotesImportReport = {
  imported: 3,
  updated: 1,
  preserved: ['note.md'],
  skipped: [{ source_id: 'b', reason: 'password protected' }],
}

describe('useAppleNotesImport', () => {
  beforeEach(() => {
    invokeMock.mockReset()
    trackEventMock.mockReset()
  })

  it('starts idle', () => {
    const { result } = renderHook(() => useAppleNotesImport('/vault'))
    expect(result.current.status).toBe('idle')
    expect(result.current.report).toBeNull()
  })

  it('imports and reports completion with safe metadata', async () => {
    invokeMock.mockResolvedValue(REPORT)
    const { result } = renderHook(() => useAppleNotesImport('/vault'))

    await act(async () => {
      await result.current.runImport()
    })

    expect(invokeMock).toHaveBeenCalledWith('import_apple_notes', { vaultPath: '/vault' })
    expect(result.current.status).toBe('done')
    expect(result.current.report).toEqual(REPORT)
    expect(trackEventMock).toHaveBeenCalledWith('apple_notes_import_completed', {
      imported: 3,
      updated: 1,
      preserved: 1,
      skipped: 1,
    })
  })

  it('flags a Full Disk Access failure', async () => {
    invokeMock.mockRejectedValue('FDA_REQUIRED: Tolaria needs Full Disk Access')
    const { result } = renderHook(() => useAppleNotesImport('/vault'))

    await act(async () => {
      await result.current.runImport()
    })

    expect(result.current.status).toBe('error')
    expect(result.current.fdaRequired).toBe(true)
    expect(trackEventMock).toHaveBeenCalledWith('apple_notes_import_failed', { fda_required: 1 })
  })

  it('reports a generic failure without the FDA flag', async () => {
    invokeMock.mockRejectedValue('disk full')
    const { result } = renderHook(() => useAppleNotesImport('/vault'))

    await act(async () => {
      await result.current.runImport()
    })

    expect(result.current.status).toBe('error')
    expect(result.current.fdaRequired).toBe(false)
    expect(result.current.error).toBe('disk full')
  })

  it('resets back to idle', async () => {
    invokeMock.mockResolvedValue(REPORT)
    const { result } = renderHook(() => useAppleNotesImport('/vault'))

    await act(async () => {
      await result.current.runImport()
    })
    act(() => {
      result.current.reset()
    })

    expect(result.current.status).toBe('idle')
    expect(result.current.report).toBeNull()
  })
})
