/**
 * SuperOps 项目宠物「超猫 SuperCat」—— e-cat 框架的血统。
 * 项圈 LED 与表情由 state 驱动：ok / degraded / alert，
 * 与后端熔断器、告警级别一一对应（详见 docs/images/pet.svg）。
 */
export type PetState = 'ok' | 'degraded' | 'alert';

const LED: Record<PetState, string> = {
  ok: '#22c55e',        // 熔断器闭合，限流正常
  degraded: '#f59e0b',  // 半开探针 / 后端回退
  alert: '#ef4444',     // 熔断打开，请求 503
};

const EYE: Record<PetState, { cy: number; ry: number; iris: string; pr: number; glint: [number, number] }> = {
  ok: { cy: 33, ry: 5.2, iris: '#2563eb', pr: 1.5, glint: [31.2, 1.3] },
  degraded: { cy: 34, ry: 3.4, iris: '#2563eb', pr: 1.3, glint: [32.6, 1.1] },
  alert: { cy: 33.5, ry: 5.4, iris: '#ef4444', pr: 1.2, glint: [31.4, 1.2] },
};

function Eye({ cx, state }: { cx: number; state: PetState }) {
  const e = EYE[state];
  return (
    <g>
      <ellipse cx={cx} cy={e.cy} rx={4.6} ry={e.ry} fill="#ffffff" stroke="#0f172a" strokeWidth={1.6} />
      <circle cx={cx} cy={e.cy + 0.5} r={2.6} fill={e.iris} />
      <circle cx={cx} cy={e.cy + 0.5} r={e.pr} fill="#0f172a" />
      <circle cx={cx - 1.3} cy={e.glint[0]} r={e.glint[1]} fill="#ffffff" />
    </g>
  );
}

export default function SuperPet({
  size = 32,
  state = 'ok',
  title,
  style,
}: {
  size?: number;
  state?: PetState;
  title?: string;
  style?: React.CSSProperties;
}) {
  // 熔断态：耳朵向后贴（警示），健康/降级：耳朵竖起（天线工作）
  const flat = state === 'alert';
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" role="img"
      aria-label={title || `SuperOps 超猫（${state}）`} style={style}>
      <title>{title || `SuperOps 超猫（${state}）`}</title>
      <rect width="64" height="64" rx="14" fill="#f8fafc" />
      <g transform={flat ? 'rotate(-30, 22.5, 17)' : undefined}>
        <path d="M 16 22 L 12 3 L 29 12 Z" fill="#f1f5f9" stroke="#0f172a" strokeWidth="2.2" strokeLinejoin="round" />
        <path d="M 19 20 L 16 8 L 26 13 Z" fill="#cbd5e1" />
      </g>
      <g transform={flat ? 'rotate(30, 41.5, 17)' : undefined}>
        <path d="M 48 22 L 52 3 L 35 12 Z" fill="#f1f5f9" stroke="#0f172a" strokeWidth="2.2" strokeLinejoin="round" />
        <path d="M 45 20 L 48 8 L 38 13 Z" fill="#cbd5e1" />
      </g>
      <ellipse cx="32" cy="34" rx="20" ry="18" fill="#f1f5f9" stroke="#0f172a" strokeWidth="2.2" />
      <Eye cx={25} state={state} />
      <Eye cx={39} state={state} />
      {state === 'alert' && (
        <g stroke="#0f172a" strokeWidth="1.8" strokeLinecap="round">
          <path d="M 21 26 L 28 27.5" />
          <path d="M 43 26 L 36 27.5" />
        </g>
      )}
      <path d="M 29.5 41 Q 32 39 34.5 41 L 32 43.5 Z" fill="#f9a8d4" />
      {state === 'ok' && (
        <g stroke="#64748b" strokeWidth="1.3" fill="none" strokeLinecap="round">
          <path d="M 32 43.5 Q 32 46 29.5 46" />
          <path d="M 32 43.5 Q 32 46 34.5 46" />
        </g>
      )}
      {state === 'degraded' && <path d="M 29.5 46 L 34.5 46" stroke="#64748b" strokeWidth="1.4" strokeLinecap="round" />}
      {state === 'alert' && <path d="M 29 46 Q 32 43.5 35 46" stroke="#64748b" strokeWidth="1.4" fill="none" strokeLinecap="round" />}
      <rect x="17" y="50" width="30" height="7" rx="3.5" fill="#2563eb" stroke="#1e40af" strokeWidth="1.2" />
      <circle cx="42" cy="53.5" r="2.6" fill={LED[state]} stroke="#ffffff" strokeWidth="0.8" />
    </svg>
  );
}
