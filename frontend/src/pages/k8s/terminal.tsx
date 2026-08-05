import { useRef, useState } from 'react';
import { Card, Form, Input, Button, Space } from 'antd';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { WebLinksAddon } from 'xterm-addon-web-links';
import 'xterm/css/xterm.css';

export default function TerminalPage() {
  const ref = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);

  const connect = (v: { cluster: string; namespace: string; pod: string }) => {
    const term = new Terminal({ fontSize: 14, fontFamily: 'Menlo,monospace', cursorBlink: true, theme: { background: '#1a1a2e', foreground: '#e0e0e0' } });
    const fit = new FitAddon(); term.loadAddon(fit); term.loadAddon(new WebLinksAddon());
    if (ref.current) { term.open(ref.current); fit.fit(); }

    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${proto}//${location.host}/api/k8s/clusters/${v.cluster}/pods/${v.namespace}/${v.pod}/exec`);
    ws.onopen = () => setConnected(true);
    ws.onmessage = (e) => term.write(e.data);
    ws.onclose = () => { setConnected(false); term.dispose(); };
    ws.onerror = () => { setConnected(false); term.dispose(); };
    term.onData((d) => { if (ws.readyState === WebSocket.OPEN) ws.send(d); });
    wsRef.current = ws;
    window.addEventListener('resize', () => fit.fit());
  };

  return (
    <Card title="Web Terminal">
      <Space style={{ marginBottom: 16 }}>
        <Form layout="inline" onFinish={connect}>
          <Form.Item name="cluster" rules={[{ required: true }]}><Input placeholder="集群 ID" style={{ width: 200 }} /></Form.Item>
          <Form.Item name="namespace" rules={[{ required: true }]}><Input placeholder="命名空间" style={{ width: 160 }} /></Form.Item>
          <Form.Item name="pod" rules={[{ required: true }]}><Input placeholder="Pod 名称" style={{ width: 200 }} /></Form.Item>
          <Form.Item><Button type="primary" htmlType="submit" disabled={connected}>连接</Button></Form.Item>
        </Form>
        {connected && <Button danger onClick={() => wsRef.current?.close()}>断开</Button>}
      </Space>
      <div ref={ref} style={{ height: 500, width: '100%' }} />
    </Card>
  );
}
