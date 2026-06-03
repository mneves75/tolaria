import { useId, useState, useRef, useEffect } from 'react'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { translate, type AppLocale } from '../lib/i18n'

const BUILT_IN_TYPES = [
  'Note',
  'Project',
  'Experiment',
  'Responsibility',
  'Procedure',
  'Person',
  'Event',
  'Topic',
] as const

export type NoteType = (typeof BUILT_IN_TYPES)[number]

interface CreateNoteDialogProps {
  open: boolean
  onClose: () => void
  onCreate: (title: string, type: string) => void
  defaultType?: string
  /** Custom types from the vault (Type documents not in built-in list) */
  customTypes?: string[]
  locale?: AppLocale
}

export function CreateNoteDialog({ open, onClose, onCreate, defaultType, customTypes = [], locale = 'en' }: CreateNoteDialogProps) {
  const [title, setTitle] = useState('')
  const [type, setType] = useState<string>('Note')
  const inputRef = useRef<HTMLInputElement>(null)
  const titleInputId = useId()

  useEffect(() => {
    if (open) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- reset on dialog open
      setTitle(''); setType(defaultType ?? 'Note')
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [open, defaultType])

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    const trimmed = title.trim()
    if (!trimmed) return
    onCreate(trimmed, type)
    onClose()
  }

  return (
    <Dialog open={open} onOpenChange={(isOpen) => { if (!isOpen) onClose() }}>
      <DialogContent showCloseButton={false} className="sm:max-w-[420px]">
        <DialogHeader>
          <DialogTitle>{translate(locale, 'createNote.title')}</DialogTitle>
          <DialogDescription className="sr-only">
            {translate(locale, 'createNote.description')}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-1.5">
            <label htmlFor={titleInputId} className="text-xs font-medium text-muted-foreground">
              {translate(locale, 'createNote.titleLabel')}
            </label>
            <Input
              id={titleInputId}
              ref={inputRef}
              placeholder={translate(locale, 'createNote.titlePlaceholder')}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </div>
          <div className="space-y-1.5">
            <div className="text-xs font-medium text-muted-foreground">
              {translate(locale, 'createNote.typeLabel')}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {BUILT_IN_TYPES.map((t) => (
                <Button
                  key={t}
                  type="button"
                  variant={type === t ? 'default' : 'outline'}
                  size="sm"
                  className={cn(
                    "rounded-full text-xs",
                    type === t && "bg-primary text-primary-foreground"
                  )}
                  onClick={() => setType(t)}
                >
                  {t}
                </Button>
              ))}
              {customTypes.map((t) => (
                <Button
                  key={t}
                  type="button"
                  variant={type === t ? 'default' : 'outline'}
                  size="sm"
                  className={cn(
                    "rounded-full text-xs",
                    type === t
                      ? "bg-[var(--accent-blue)] text-primary-foreground"
                      : "border-[var(--accent-blue)] text-[var(--accent-blue)]"
                  )}
                  onClick={() => setType(t)}
                >
                  {t}
                </Button>
              ))}
            </div>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              {translate(locale, 'common.cancel')}
            </Button>
            <Button type="submit" disabled={!title.trim()}>
              {translate(locale, 'common.create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
