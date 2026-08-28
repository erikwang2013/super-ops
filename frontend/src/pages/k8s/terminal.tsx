import { useEffect, useRef, useState } from 'react';
import { Card, Form, Input, Button, Space, Modal } from 'antd';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { WebLinksAddon } from 'xterm-addon-web-links';
import 'xterm/css/xterm.css';
import { useAuthStore } from '../../stores/auth';

export default function TerminalPage() {
  const ref = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    return () => {
      wsRef.current?.close();
      termRef.current?.dispose();
      wsRef.current = null;
      termRef.current = null;
    };
  }, []);

  const doConnect = (v: { cluster: string; namespace: string; pod: string; container?: string }) => {
    wsRef.current?.close();
    termRef.current?.dispose();

    const term = new Terminal({ fontSize: 14, fontFamily: 'Menlo,monospace', cursorBlink: true, theme: { background: '#1a1a2e', foreground: '#e0e0e0' } });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    termRef.current = term;
    if (ref.current) { term.open(ref.current); fit.fit(); }

    const token = useAuthStore.getState().token;
    const container = v.container ? `&container=${encodeURIComponent(v.container)}` : '';
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    // B3 会话管控：连接前用户已确认，携带 confirm=1（gateway 校验，缺失则 400）
    const ws = new WebSocket(`${proto}//${location.host}/api/k8s/clusters/${v.cluster}/pods/${v.namespace}/${v.pod}/exec?token=${encodeURIComponent(token || '')}${container}&confirm=1`);
    // xterm 接收二进制更快且不受 utf-8 拆分影响
    ws.binaryType = 'arraybuffer';
    wsRef.current = ws;

    const sendResize = () => {
      fit.fit();
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));
      }
    };
    const onResize = () => sendResize();
    ws.onopen = () => { setConnected(true); sendResize(); };
    ws.onmessage = (e) => term.write(new Uint8Array(e.data));
    ws.onclose = () => {
      setConnected(false);
      term.dispose();
      termRef.current = null;
      window.removeEventListener('resize', onResize);
    };
    ws.onerror = () => {
      setConnected(false);
      term.dispose();
      termRef.current = null;
      window.removeEventListener('resize', onResize);
    };
    term.onData((d) => { if (ws.readyState === WebSocket.OPEN) ws.send(d); });
    window.addEventListener('resize', onResize);
  };

  const connect = (v: { cluster: string; namespace: string; pod: string; container?: string }) => {
    Modal.confirm({
      title: '终端会话安全确认',
      content: `将建立到 ${v.cluster}/${v.namespace}/${v.pod} 的交互式终端会话（最长 30 分钟，全程录制）。是否继续？`,
      okText: '连接',
      cancelText: '取消',
      onOk: () => doConnect(v),
    });
  };

  return (
    <Card title="Web Terminal">
      <Space style={{ marginBottom: 16 }}>
        <Form layout="inline" onFinish={connect}>
          <Form.Item name="cluster" rules={[{ required: true }]}><Input placeholder="集群 ID" style={{ width: 200 }} /></Form.Item>
          <Form.Item name="namespace" rules={[{ required: true }]}><Input placeholder="命名空间" style={{ width: 160 }} /></Form.Item>
          <Form.Item name="pod" rules={[{ required: true }]}><Input placeholder="Pod 名称" style={{ width: 200 }} /></Form.Item>
          <Form.Item name="container" rules={[{ required: false }]}><Input placeholder="容器（可选）" style={{ width: 160 }} /></Form.Item>
          <Form.Item><Button type="primary" htmlType="submit" disabled={connected}>连接</Button></Form.Item>
        </Form>
        {connected && <Button danger onClick={() => wsRef.current?.close()}>断开</Button>}
      </Space>
      <div ref={ref} style={{ height: 500, width: '100%' }} />
    </Card>
  );
}
