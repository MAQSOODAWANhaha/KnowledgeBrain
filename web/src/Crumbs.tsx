export type Crumb = { label: string; href?: string };

function hrefOf(href: string): string {
  return href.startsWith("#") ? href : `#${href}`;
}

export function Crumbs({ items }: { items: Crumb[] }) {
  return (
    <>
      {items.map((item, i) => {
        const last = i === items.length - 1;
        return (
          <span className="crumb" key={`${item.href ?? ""}:${item.label}:${i}`}>
            {i > 0 ? (
              <svg className="crumb-sep" viewBox="0 0 16 16" aria-hidden="true">
                <path d="M6 3.5 10.5 8 6 12.5" />
              </svg>
            ) : null}
            {last || !item.href ? (
              <span className={last ? "crumb-now" : undefined} aria-current={last ? "page" : undefined} title={item.label}>
                {item.label}
              </span>
            ) : (
              <a href={hrefOf(item.href)} title={item.label}>
                {item.label}
              </a>
            )}
          </span>
        );
      })}
    </>
  );
}
