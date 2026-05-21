import CodeMirror, { type Extension } from "@uiw/react-codemirror";
import { markdown } from "@codemirror/lang-markdown";
import { json } from "@codemirror/lang-json";
import { StreamLanguage } from "@codemirror/language";
import { shell } from "@codemirror/legacy-modes/mode/shell";
import { useMemo } from "react";

export type CodeEditorLanguage = "markdown" | "json" | "shell";

export interface CodeEditorProps {
  value: string;
  onChange: (value: string) => void;
  language: CodeEditorLanguage;
  readOnly?: boolean;
  placeholder?: string;
  className?: string;
}

/** Map language prop → CM6 language extension. */
function languageExtension(lang: CodeEditorLanguage): Extension {
  switch (lang) {
    case "markdown":
      return markdown();
    case "json":
      return json();
    case "shell":
      return StreamLanguage.define(shell);
  }
}

/**
 * CodeEditor — the single editing surface for claude-deck app/web.
 *
 * Wraps CodeMirror 6 via @uiw/react-codemirror for a controlled, typed
 * component. The uiw wrapper handles EditorView lifecycle and controlled
 * value/onChange sync, which is error-prone to hand-wire directly.
 *
 * Theming: uiw/react-codemirror reads the `theme` prop and applies a
 * built-in "light" or "dark" CM6 theme. Dark mode is detected by reading
 * the `.dark` class on <html> at render time. app/web has no theme toggle
 * yet — task B1 builds one. When B1 adds a theme system it must re-render
 * CodeEditor consumers on toggle (e.g. via a store or context value change);
 * if it does not, CodeEditor will need its own theme subscription at that
 * point.
 */
export function CodeEditor({
  value,
  onChange,
  language,
  readOnly = false,
  placeholder,
  className,
}: CodeEditorProps) {
  // Read the .dark class on <html> at render time. No MutationObserver;
  // no theme system exists in app/web yet. When B1 introduces one, any
  // state/context change driving theme will naturally re-render this
  // component and pick up the updated class.
  const isDark =
    typeof document !== "undefined" &&
    document.documentElement.classList.contains("dark");

  const extensions = useMemo<Extension[]>(
    () => [languageExtension(language)],
    [language],
  );

  return (
    <CodeMirror
      value={value}
      onChange={onChange}
      extensions={extensions}
      theme={isDark ? "dark" : "light"}
      readOnly={readOnly}
      placeholder={placeholder}
      className={className}
      basicSetup={{
        lineNumbers: true,
        foldGutter: false,
        dropCursor: false,
        allowMultipleSelections: false,
        indentOnInput: true,
        syntaxHighlighting: true,
        bracketMatching: true,
        closeBrackets: true,
        autocompletion: true,
        rectangularSelection: false,
        crosshairCursor: false,
        highlightActiveLine: true,
        highlightActiveLineGutter: true,
        highlightSelectionMatches: false,
        searchKeymap: false,
        foldKeymap: false,
        completionKeymap: true,
        lintKeymap: false,
      }}
    />
  );
}
