import { useId, useState } from 'react'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { translate, type AppLocale } from '../lib/i18n'

interface CreateTypeDialogProps {
  open: boolean
  onClose: () => void
  onCreate: (name: string) => boolean | void | Promise<boolean | void>
  initialName?: string
  locale?: AppLocale
}

interface CreateTypeDialogFormProps {
  initialName: string
  locale: AppLocale
  onClose: () => void
  onCreate: (name: string) => boolean | void | Promise<boolean | void>
}

function CreateTypeDialogForm({ initialName, locale, onClose, onCreate }: CreateTypeDialogFormProps) {
  const [name, setName] = useState(initialName)
  const nameInputId = useId()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    const trimmed = name.trim()
    if (!trimmed) return
    const created = await onCreate(trimmed)
    if (created !== false) onClose()
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <label htmlFor={nameInputId} className="text-xs font-medium text-muted-foreground">
          {translate(locale, 'createType.nameLabel')}
        </label>
        <Input
          id={nameInputId}
          autoFocus
          placeholder={translate(locale, 'createType.namePlaceholder')}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onFocus={(e) => e.currentTarget.select()}
        />
        <p className="text-xs text-muted-foreground">
          {translate(locale, 'createType.helpText')}
        </p>
      </div>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onClose}>
          {translate(locale, 'common.cancel')}
        </Button>
        <Button type="submit" disabled={!name.trim()}>
          {translate(locale, 'common.create')}
        </Button>
      </DialogFooter>
    </form>
  )
}

export function CreateTypeDialog({ open, onClose, onCreate, initialName = '', locale = 'en' }: CreateTypeDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(isOpen) => { if (!isOpen) onClose() }}>
      <DialogContent showCloseButton={false} className="sm:max-w-[380px]">
        <DialogHeader>
          <DialogTitle>{translate(locale, 'createType.title')}</DialogTitle>
          <DialogDescription>
            {translate(locale, 'createType.description')}
          </DialogDescription>
        </DialogHeader>
        <CreateTypeDialogForm
          key={initialName}
          initialName={initialName}
          locale={locale}
          onClose={onClose}
          onCreate={onCreate}
        />
      </DialogContent>
    </Dialog>
  )
}
