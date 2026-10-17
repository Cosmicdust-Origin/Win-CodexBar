import { brandColorOnDark, getProviderIcon } from "./providerIcons";

interface Props {
  providerId: string;
  size?: number;
  className?: string;
  title?: string;
}

/**
 * Renders a provider brand icon. If the registry has an SVG, inline it inside
 * a rounded badge tinted with the brand color; otherwise render a circular
 * badge containing a fallback letter.
 */
export function ProviderIcon({
  providerId,
  size = 22,
  className,
  title,
}: Props) {
  const entry = getProviderIcon(providerId);
  const onDark = brandColorOnDark(entry.brandColor);
  const tint = {
    ["--provider-brand" as string]: entry.brandColor,
    // Only near-black brands get a dark-surface tint; CSS picks it on dark
    // surfaces and keeps the brand color on light ones.
    ...(onDark ? { ["--provider-brand-dark" as string]: onDark } : {}),
  };
  const dims = { width: size, height: size };

  if (entry.svgPath) {
    return (
      <span
        className={`provider-icon provider-icon--svg${className ? " " + className : ""}`}
        style={{
          ...dims,
          ...tint,
        }}
        title={title}
        aria-hidden={title ? undefined : true}
        // eslint-disable-next-line react/no-danger -- SVGs are bundled locally, no user input.
        dangerouslySetInnerHTML={{ __html: entry.svgPath }}
      />
    );
  }

  return (
    <span
      className={`provider-icon provider-icon--letter${className ? " " + className : ""}`}
      style={{
        ...dims,
        ...tint,
      }}
      title={title}
      aria-hidden={title ? undefined : true}
    >
      {entry.fallbackLetter}
    </span>
  );
}
