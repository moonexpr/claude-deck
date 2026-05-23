import { useState } from 'react';
import { Save, X, Trash2, ChevronDown, ChevronUp, HelpCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { MarkdownPreviewToggle } from '@/components/shared/MarkdownPreviewToggle';
import { AVAILABLE_TOOLS } from '@/types/commands';
import type { SlashCommand } from '@/types/commands';

interface CommandEditorProps {
  command: SlashCommand;
  onSave: (command: SlashCommand) => void;
  onDelete: (command: SlashCommand) => void;
  onCancel: () => void;
}

export function CommandEditor({ command, onSave, onDelete, onCancel }: CommandEditorProps) {
  const [description, setDescription] = useState(command.description || '');
  const [allowedTools, setAllowedTools] = useState<string[]>(command.allowed_tools || []);
  const [content, setContent] = useState(command.content);
  const [saving, setSaving] = useState(false);
  const [frontmatterOpen, setFrontmatterOpen] = useState(true);
  const [showToolHelp, setShowToolHelp] = useState(false);
  const [showPlaceholderHelp, setShowPlaceholderHelp] = useState(false);

  const handleToolToggle = (tool: string) => {
    if (allowedTools.includes(tool)) {
      setAllowedTools(allowedTools.filter((t) => t !== tool));
    } else {
      setAllowedTools([...allowedTools, tool]);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await onSave({
        ...command,
        description,
        allowed_tools: allowedTools.length > 0 ? allowedTools : undefined,
        content,
      });
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = () => {
    onDelete(command);
  };

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="space-y-2">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-2xl font-bold">{command.name}</h2>
            <p className="text-sm text-muted-foreground">{command.path}</p>
          </div>
          <Badge variant={command.scope === 'user' ? 'default' : 'secondary'}>
            {command.scope}
          </Badge>
        </div>
      </div>

      {/* Frontmatter Section */}
      <Card>
        <CardHeader
          className="cursor-pointer"
          onClick={() => setFrontmatterOpen(!frontmatterOpen)}
        >
          <CardTitle className="flex items-center justify-between text-lg">
            <span>Frontmatter (YAML Metadata)</span>
            {frontmatterOpen ? <ChevronUp className="h-5 w-5" /> : <ChevronDown className="h-5 w-5" />}
          </CardTitle>
        </CardHeader>
        {frontmatterOpen && (
          <CardContent className="space-y-4">
            {/* Description */}
            <div className="space-y-2">
              <Label htmlFor="description">Description (Optional)</Label>
              <Input
                id="description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Brief description of what this command does"
              />
            </div>

            {/* Allowed Tools */}
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <Label>Allowed Tools (Optional)</Label>
                <HelpCircle
                  className="h-4 w-4 cursor-pointer text-muted-foreground hover:text-foreground"
                  onClick={() => setShowToolHelp(!showToolHelp)}
                />
              </div>
              {showToolHelp && (
                <Card className="bg-muted">
                  <CardContent className="pt-4 text-sm">
                    <p>Select which tools this command can use. If none selected, all tools are allowed.</p>
                  </CardContent>
                </Card>
              )}
              <div className="flex flex-wrap gap-2">
                {AVAILABLE_TOOLS.map((tool) => (
                  <Badge
                    key={tool}
                    variant={allowedTools.includes(tool) ? 'default' : 'outline'}
                    className="cursor-pointer"
                    onClick={() => handleToolToggle(tool)}
                  >
                    {tool}
                  </Badge>
                ))}
              </div>
            </div>
          </CardContent>
        )}
      </Card>

      {/* Markdown Content Section */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle className="text-lg">Command Content (Markdown)</CardTitle>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setShowPlaceholderHelp(!showPlaceholderHelp)}
            >
              <HelpCircle className="h-4 w-4 mr-2" />
              Placeholders
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {showPlaceholderHelp && (
            <Card className="bg-muted">
              <CardContent className="pt-4 text-sm space-y-2">
                <p className="font-semibold">Available Argument Placeholders:</p>
                <ul className="list-disc list-inside space-y-1">
                  <li><code className="bg-background px-1">$ARGUMENTS</code> - All arguments passed to the command</li>
                  <li><code className="bg-background px-1">$1, $2, $3...</code> - Individual positional arguments</li>
                  <li><code className="bg-background px-1">$@</code> - All arguments as separate strings</li>
                </ul>
                <p className="mt-2">Example: <code className="bg-background px-1">/review src/app.ts</code> {'->'} $ARGUMENTS = "src/app.ts"</p>
              </CardContent>
            </Card>
          )}
          <MarkdownPreviewToggle
            value={content}
            onChange={setContent}
            placeholder={`Write your command instructions here...\n\nUsage: /command-name <args>\n\nYou can use markdown formatting:\n- **Bold text**\n- *Italic text*\n- \`Code blocks\`\n- Lists and more`}
            minHeight="320px"
          />
        </CardContent>
      </Card>

      {/* Actions */}
      <div className="flex items-center justify-between pt-4 border-t">
        <Button variant="destructive" onClick={handleDelete}>
          <Trash2 className="h-4 w-4 mr-2" />
          Delete Command
        </Button>
        <div className="flex gap-2">
          <Button variant="outline" onClick={onCancel}>
            <X className="h-4 w-4 mr-2" />
            Cancel
          </Button>
          <Button onClick={handleSave} disabled={saving}>
            <Save className="h-4 w-4 mr-2" />
            {saving ? 'Saving...' : 'Save Changes'}
          </Button>
        </div>
      </div>
    </div>
  );
}
