/** Brains brand mark: two hemispheres plus the spark accent — matches the
 * favicon (public/favicon.svg). Scales via className; tone follows
 * currentColor for the strokes, primary for the spark. */
export function BrandMark({ className = "size-4" }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" aria-hidden className={cnBrand(className)}>
      <path
        d="M12.4 7.2c-1.9.9-2.7 3-2 4.8-1.8.7-2.9 2.6-2.5 4.5.3 1.4 1.3 2.5 2.6 3-.3 2 1.1 3.8 3.1 4.1 1.1.2 2.2-.2 3-1V8.4c-.9-1.1-2.5-1.7-4.2-1.2Z"
        fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"
      />
      <path
        d="M19.6 7.2c1.9.9 2.7 3 2 4.5 1.8.7 2.9 2.6 2.5 4.5-.3 1.4-1.3 2.5-2.6 3 .3 2-1.1 3.8-3.1 4.1"
        fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" opacity="0.65"
      />
      <path d="M24.5 4.5l.9 2.1 2.1.9-2.1.9-.9 2.1-.9-2.1-2.1-.9 2.1-.9.9-2.1Z" fill="var(--primary)" />
    </svg>
  );
}

function cnBrand(className: string): string {
  return `shrink-0 ${className}`;
}
