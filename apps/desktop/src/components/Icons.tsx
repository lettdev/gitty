// A small consistent icon set (16px grid, 1.4px strokes) drawn inline.
import type { SVGProps } from "react";

type P = SVGProps<SVGSVGElement> & { size?: number };
const base = (size = 14) => ({ width: size, height: size, viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round" as const, strokeLinejoin: "round" as const, "aria-hidden": true });

export const BranchIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><circle cx="5" cy="4" r="1.6" /><circle cx="5" cy="12" r="1.6" /><circle cx="11" cy="6" r="1.6" /><path d="M5 5.6v4.8M11 7.6c0 2.4-6 1.6-6 2.8" /></svg>
);
export const FolderIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M2 4.5h4l1.5 1.5H14v6.5H2z" /></svg>
);
export const CloudIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M4 11.5h8a2.5 2.5 0 0 0 .3-5A3.5 3.5 0 0 0 5.5 6 2.8 2.8 0 0 0 4 11.5z" /></svg>
);
export const StashIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><rect x="2.5" y="3" width="11" height="3" rx="1" /><path d="M3.5 6v6.5h9V6M6.5 9h3" /></svg>
);
export const TagIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M8.5 2.5h5v5L8 13 3 8z" /><circle cx="11" cy="5" r=".8" /></svg>
);
export const SearchIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><circle cx="7" cy="7" r="4.5" /><path d="m10.5 10.5 3 3" /></svg>
);
export const ChevronDown = ({ size = 10, ...p }: P) => (
  <svg {...base(size)} viewBox="0 0 12 12" {...p} strokeWidth={1.5}><path d="M3 4.5 6 7.5 9 4.5" /></svg>
);
export const ChevronRight = ({ size = 10, ...p }: P) => (
  <svg {...base(size)} viewBox="0 0 12 12" {...p} strokeWidth={1.5}><path d="M4.5 3 7.5 6 4.5 9" /></svg>
);
export const SyncIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p} strokeWidth={1.5}><path d="M13 6.5A5 5 0 0 0 3.6 5M3 9.5A5 5 0 0 0 12.4 11" /><path d="M3.2 2.5v2.8H6M12.8 13.5v-2.8H10" /></svg>
);
export const GearIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><circle cx="8" cy="8" r="2" /><path d="M8 1.8v2M8 12.2v2M1.8 8h2M12.2 8h2M3.6 3.6 5 5M11 11l1.4 1.4M3.6 12.4 5 11M11 5l1.4-1.4" /></svg>
);
export const IgnoreIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><circle cx="8" cy="8" r="5.5" /><path d="m4.2 11.8 7.6-7.6" /></svg>
);
export const TreeIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M3 3h4M5 3v9h3M5 7.5h3M10 7.5h3M10 12h3" /></svg>
);
export const ListIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M3 4h10M3 8h10M3 12h10" /></svg>
);
export const PlusIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M8 3v10M3 8h10" /></svg>
);
export const CommitIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><circle cx="8" cy="8" r="2.6" /><path d="M1.5 8h3.9M10.6 8h3.9" /></svg>
);
export const DownloadIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M8 2.5v7.5M5 7l3 3 3-3M3 12.5h10" /></svg>
);
export const FolderOpenIcon = ({ size, ...p }: P) => (
  <svg {...base(size)} {...p}><path d="M2 12.5V4.5h4l1.5 1.5H13v1.5M2 12.5l1.8-5h10.7l-1.8 5z" /></svg>
);
