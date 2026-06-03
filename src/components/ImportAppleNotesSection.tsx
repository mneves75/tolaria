import { useState } from 'react'
import { openUrl } from '@tauri-apps/plugin-opener'

import type { TranslationKey, TranslationValues } from '../lib/i18n'
import { useAppleNotesImport } from '../hooks/useAppleNotesImport'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { SectionHeading, SettingsGroup, SettingsRow } from './SettingsControls'

type Translate = (key: TranslationKey, values?: TranslationValues) => string

// System Settings → Privacy & Security → Full Disk Access.
const FDA_SETTINGS_URL = 'x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles'

interface ImportAppleNotesSectionProps {
  t: Translate
  vaultPath: string
}

/**
 * Settings entry that imports Apple Notes into the current vault. macOS-only;
 * callers gate rendering on the platform.
 */
export function ImportAppleNotesSection({ t, vaultPath }: ImportAppleNotesSectionProps) {
  const [open, setOpen] = useState(false)
  const importer = useAppleNotesImport(vaultPath)

  const close = () => {
    setOpen(false)
    importer.reset()
  }

  return (
    <>
      <SectionHeading title={t('settings.appleNotes.heading')} />
      <SettingsGroup>
        <SettingsRow
          label={t('settings.appleNotes.title')}
          description={t('settings.appleNotes.description')}
          controlWidth="auto"
          testId="apple-notes-import-row"
        >
          <Button
            variant="outline"
            onClick={() => setOpen(true)}
            data-testid="apple-notes-import-open"
          >
            {t('settings.appleNotes.action')}
          </Button>
        </SettingsRow>
      </SettingsGroup>

      <Dialog open={open} onOpenChange={(next) => { if (!next) close() }}>
        {/* The Settings panel is a custom modal at z-[1300]; lift this dialog above it. */}
        <DialogContent data-testid="apple-notes-import-dialog" className="z-[1400]">
          <ImportDialogBody t={t} importer={importer} onClose={close} />
        </DialogContent>
      </Dialog>
    </>
  )
}

interface ImportDialogBodyProps {
  t: Translate
  importer: ReturnType<typeof useAppleNotesImport>
  onClose: () => void
}

function ImportDialogBody({ t, importer, onClose }: ImportDialogBodyProps) {
  if (importer.status === 'done' && importer.report) {
    const { report } = importer
    return (
      <>
        <DialogHeader>
          <DialogTitle>{t('settings.appleNotes.doneTitle')}</DialogTitle>
          <DialogDescription data-testid="apple-notes-import-summary">
            {t('settings.appleNotes.summary', {
              imported: report.imported,
              updated: report.updated,
              preserved: report.preserved.length,
              skipped: report.skipped.length,
            })}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button onClick={onClose}>{t('settings.appleNotes.close')}</Button>
        </DialogFooter>
      </>
    )
  }

  if (importer.status === 'error') {
    return (
      <>
        <DialogHeader>
          <DialogTitle>{t('settings.appleNotes.errorTitle')}</DialogTitle>
          <DialogDescription data-testid="apple-notes-import-error">
            {importer.fdaRequired ? t('settings.appleNotes.fdaBody') : importer.error}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          {importer.fdaRequired ? (
            <Button onClick={() => { void openUrl(FDA_SETTINGS_URL) }}>
              {t('settings.appleNotes.openSettings')}
            </Button>
          ) : null}
          <Button variant="outline" onClick={onClose}>
            {t('settings.appleNotes.close')}
          </Button>
        </DialogFooter>
      </>
    )
  }

  const running = importer.status === 'running'
  return (
    <>
      <DialogHeader>
        <DialogTitle>{t('settings.appleNotes.dialogTitle')}</DialogTitle>
        <DialogDescription>{t('settings.appleNotes.dialogIntro')}</DialogDescription>
      </DialogHeader>
      <p className="text-xs leading-5 text-muted-foreground">{t('settings.appleNotes.consent')}</p>
      <DialogFooter>
        <Button
          onClick={() => { void importer.runImport() }}
          disabled={running}
          data-testid="apple-notes-import-start"
        >
          {running ? t('settings.appleNotes.importing') : t('settings.appleNotes.start')}
        </Button>
        <Button variant="outline" onClick={onClose} disabled={running}>
          {t('settings.appleNotes.cancel')}
        </Button>
      </DialogFooter>
    </>
  )
}
