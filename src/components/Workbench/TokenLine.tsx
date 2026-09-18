import { tokenStyle, type ThemedToken } from "../../lib/highlight";

/** One line of highlighted code as coloured spans. */
export default function TokenLine({ tokens }: { tokens: ThemedToken[] }) {
  return (
    <>
      {tokens.map((t, i) => (
        <span key={i} style={tokenStyle(t)}>
          {t.content}
        </span>
      ))}
    </>
  );
}
