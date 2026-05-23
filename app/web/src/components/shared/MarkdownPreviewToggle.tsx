import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { MarkdownRenderer } from './MarkdownRenderer'
import { CodeEditor } from './CodeEditor'

interface MarkdownPreviewToggleProps {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  minHeight?: string
  disabled?: boolean
  defaultTab?: 'edit' | 'preview'
}

export function MarkdownPreviewToggle({
  value,
  onChange,
  placeholder = 'Write markdown content...',
  minHeight = '300px',
  disabled = false,
  defaultTab = 'edit',
}: MarkdownPreviewToggleProps) {
  return (
    <Tabs defaultValue={defaultTab} className="w-full">
      <TabsList className="grid w-full grid-cols-2">
        <TabsTrigger value="edit">Edit</TabsTrigger>
        <TabsTrigger value="preview">Preview</TabsTrigger>
      </TabsList>
      <TabsContent value="edit" className="mt-2">
        <div style={{ minHeight }} className="overflow-hidden rounded-md border border-input">
          <CodeEditor
            language="markdown"
            value={value}
            onChange={disabled ? () => undefined : onChange}
            placeholder={placeholder}
            readOnly={disabled}
          />
        </div>
      </TabsContent>
      <TabsContent value="preview" className="mt-2">
        <div
          className="border rounded-md p-4 overflow-auto bg-muted/30"
          style={{ minHeight }}
        >
          {value ? (
            <MarkdownRenderer content={value} />
          ) : (
            <p className="text-muted-foreground italic">Nothing to preview</p>
          )}
        </div>
      </TabsContent>
    </Tabs>
  )
}
