import { Alert } from 'antd';

const DEFAULT_URL = 'http://localhost:3000';

// 嵌入 Grafana：URL 优先级 — 地址栏 ?url= > VITE_GRAFANA_URL > localhost:3000
export default function GrafanaPage() {
  const fromQuery = new URLSearchParams(window.location.search).get('url');
  const src = fromQuery || (import.meta as any).env?.VITE_GRAFANA_URL || DEFAULT_URL;
  return <>
    <Alert type="info" showIcon style={{ marginBottom: 8 }}
      message={`嵌入 Grafana：${src}`}
      description="需要 Grafana 以匿名访问或相同域登录态开放；可用 ?url= 参数覆盖，例如 /ops/grafana?url=http://grafana.example.com" />
    <iframe title="Grafana" src={src} style={{ width: '100%', height: 'calc(100vh - 200px)', border: 0 }}
      sandbox="allow-scripts allow-same-origin allow-forms allow-popups" />
  </>;
}
