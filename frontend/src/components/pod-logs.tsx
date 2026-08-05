import { useEffect, useRef } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import 'xterm/css/xterm.css';

interface PodLogsProps { clusterId: string; namespace: string; podName: string; }

export default function PodLogs({ clusterId, namespace, podName }: PodLogsProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const term = new Terminal({ fontSize: 13, fontFamily: 'Menlo, monospace', theme: { background: '#1a1a2e' } });
    const fit = new FitAddon(); term.loadAddon(fit);
    if (ref.current) { term.open(ref.current); fit.fit(); }
    term.writeln(`${namespace}/${podName} on ${clusterId}`);
    const h = () => fit.fit(); window.addEventListener('resize', h);
    return () => { term.dispose(); window.removeEventListener('resize', h); };
  }, [clusterId, namespace, podName]);
  return <div ref={ref} style={{ height: 400, width: '100%' }} />;
}
