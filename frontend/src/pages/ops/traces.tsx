import { App, Button, Card, Input, Space, Typography } from 'antd';
import { useState } from 'react';

const STORAGE_KEY = 'jaeger-base-url';
const DEFAULT_URL = 'http://localhost:16686';

export default function TracesPage() {
  const { message } = App.useApp();
  const [url, setUrl] = useState(() => localStorage.getItem(STORAGE_KEY) || DEFAULT_URL);
  const [input, setInput] = useState(url);
  const [nonce, setNonce] = useState(0);

  const save = () => {
    const v = input.trim().replace(/\/+$/, '');
    if (!/^https?:\/\/.+/.test(v)) {
      message.error('地址须以 http(s):// 开头');
      return;
    }
    localStorage.setItem(STORAGE_KEY, v);
    setUrl(v);
    setNonce(n => n + 1);
    message.success('Jaeger 地址已保存');
  };

  return (
    <Card
      title="链路追踪"
      extra={
        <Space.Compact>
          <Input style={{ width: 320 }} value={input} onChange={e => setInput(e.target.value)}
            onPressEnter={save} placeholder={DEFAULT_URL} />
          <Button type="primary" onClick={save}>保存</Button>
        </Space.Compact>
      }
    >
      <Typography.Paragraph type="secondary" style={{ marginTop: 0 }}>
        通过 iframe 嵌入 Jaeger UI（compose 默认 http://localhost:16686）。若 Jaeger 未启动，页面将显示连接失败。
      </Typography.Paragraph>
      <iframe key={nonce} title="jaeger" src={`${url}/search?service=`}
        style={{ width: '100%', height: 'calc(100vh - 260px)', border: '1px solid #f0f0f0', borderRadius: 8 }} />
    </Card>
  );
}
