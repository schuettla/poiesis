import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

export default function RunText({
  text,
  streaming,
}: {
  text: string;
  streaming?: boolean;
}) {
  return (
    <div className={`run-text${streaming ? " streaming" : ""}`}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}
