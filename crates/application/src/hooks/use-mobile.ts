import * as React from "react";

const MOBILE_BREAKPOINT = 768;

export function useIsMobile() {
  const [isMobile, setIsMobile] = React.useState<boolean | undefined>(
    undefined,
  );

  React.useEffect(() => {
		if (typeof window === "undefined") return;
    const onChange = () => {
      setIsMobile(window.innerWidth < MOBILE_BREAKPOINT);
    };
    const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`);
		const render = () => {
    mql.addEventListener("change", onChange);
	    setIsMobile(window.innerWidth < MOBILE_BREAKPOINT);
		}
		render();
    return () => mql.removeEventListener("change", onChange);
  }, []);

  return !!isMobile;
}
