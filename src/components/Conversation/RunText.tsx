import { useMemo, type ComponentProps } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { remarkClaimMarks, type Mark } from "../../lib/verify";
import "./Verify.css";

/** `USE-UI-7`: the claims of a checked answer to underline, and what a click on one does. */
export interface ClaimMarks {
  marks: Mark[];
  picked: string | null;
  onPick: (id: string) => void;
}

type MarkProps = ComponentProps<"mark"> & { node?: unknown; "data-claim"?: string };

export default function RunText({
  text,
  streaming,
  claims,
}: {
  text: string;
  streaming?: boolean;
  claims?: ClaimMarks;
}) {
  const marks = claims?.marks;
  const picked = claims?.picked ?? null;
  const onPick = claims?.onPick;

  const plugins = useMemo(
    () => (marks && marks.length > 0 ? [remarkGfm, [remarkClaimMarks, marks] as [typeof remarkClaimMarks, Mark[]]] : [remarkGfm]),
    [marks],
  );
  const components = useMemo(() => {
    if (!onPick) return undefined;
    return {
      // The only `mark` element in an answer is one this check drew, so it can
      // be a button for the claim it carries.
      mark: ({ node: _node, children, className, ...props }: MarkProps) => {
        const id = props["data-claim"];
        return (
          <mark
            {...props}
            className={`${className ?? ""}${id === picked ? " picked" : ""}`}
            role="button"
            aria-pressed={id === picked}
            onClick={() => id && onPick(id)}
            onKeyDown={(e) => {
              if (id && (e.key === "Enter" || e.key === " ")) {
                e.preventDefault();
                onPick(id);
              }
            }}
          >
            {children}
          </mark>
        );
      },
    };
  }, [onPick, picked]);

  return (
    <div className={`run-text${streaming ? " streaming" : ""}`}>
      <ReactMarkdown remarkPlugins={plugins} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
}
