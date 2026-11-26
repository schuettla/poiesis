import type { ComponentProps, ReactNode } from "react";
import AppLogo from "../Apps/AppLogo";
import { classifyLink, type LinkService } from "../../lib/linkKinds";
import "./LinkChip.css";

/** `UI-7`: which mark in `public/app-logos/` stands for each service. A service with no file
 * (`googlemaps`, `google`, `atlassian`, `obsidian`) shows the first letter of its name, as
 * `AppLogo` does for any app without a mark. `web` is not here: it is never a chip. */
export const LINK_LOGOS: Partial<Record<LinkService, string>> = {
  github: "github.svg",
  slack: "slack.svg",
  discord: "discord.svg",
  googledocs: "googledocs.svg",
  googlesheets: "googlesheets.svg",
  googleslides: "googleslides.svg",
  googleforms: "googleforms.svg",
  googledrive: "googledrive.svg",
  googlecalendar: "googlecalendar.svg",
  googlemeet: "googlemeet.svg",
  gmail: "gmail.svg",
  x: "twitter.svg",
  linkedin: "linkedin.svg",
  youtube: "youtube.svg",
  notion: "notion.svg",
  linear: "linear.svg",
  figma: "figma.svg",
  npm: "npm.svg",
  jira: "jira.svg",
  confluence: "confluence.svg",
  sentry: "sentry.svg",
  asana: "asana.svg",
  todoist: "todoist.svg",
  clickup: "clickup.svg",
  gitlab: "gitlab.svg",
};

/** The words of a link's children, when they are only text. */
function textOf(children: ReactNode): string {
  if (typeof children === "string") return children;
  if (typeof children === "number") return String(children);
  if (Array.isArray(children)) return children.map(textOf).join("");
  return "";
}

type LinkProps = ComponentProps<"a"> & { node?: unknown };

/** `UI-7`: an answer's link, as a small chip when it points at a service the app knows: the
 * service's mark and what it points at, read from the address alone (nothing is fetched). A
 * link with its own words keeps them. Any other link, and a look-alike host, stays as it is.
 * Clicking opens it the way links open everywhere else in the app: it is still an `<a>`. */
export default function LinkChip({ node: _node, href, children, ...props }: LinkProps) {
  const info = href ? classifyLink(href) : null;
  if (!href || !info || info.service === "web") {
    return (
      <a href={href} {...props}>
        {children}
      </a>
    );
  }
  const own = textOf(children).trim();
  // A bare address reads as its short name; words someone wrote stay.
  const words = own && own !== href ? children : info.label;
  const file = LINK_LOGOS[info.service];
  return (
    <a
      {...props}
      href={href}
      className="link-chip"
      title={href}
      data-service={info.service}
      data-kind={info.kind}
    >
      <AppLogo
        slug={info.service}
        name={info.label}
        src={file ? `${import.meta.env.BASE_URL}app-logos/${file}` : null}
      />
      <span className="link-chip-text">{words}</span>
    </a>
  );
}
