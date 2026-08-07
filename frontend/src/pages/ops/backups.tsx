import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { Card, Col, Row, Statistic, Tag } from 'antd';
import { BackupStatusRow, BackupSummaryRow, backupApi } from '../../services/api';

const statusColor: Record<string, string> = {
  running: 'processing', ok: 'success', failed: 'red',
};

function fmtSize(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0, v = bytes;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

export default function BackupsPage() {
  const { data: statuses, isLoading } = useQuery({
    queryKey: ['backups'],
    queryFn: () => backupApi.listStatus(),
  });
  const { data: summaries } = useQuery({
    queryKey: ['backup-summaries'],
    queryFn: () => backupApi.summary(),
  });
  const { data: objects } = useQuery({
    queryKey: ['backup-objects'],
    queryFn: () => backupApi.listObjects(),
  });
  const sum = summaries?.summaries || [];
  const stale = sum.filter((s: BackupSummaryRow) => s.age_hours > 24);
  const failed = sum.filter((s: BackupSummaryRow) => s.status === 'failed');
  return <>
    <Row gutter={[16, 16]}>
      <Col span={6}><Card><Statistic title="备份数据库数" value={sum.length} /></Card></Col>
      <Col span={6}><Card><Statistic title="最近 24h 内正常" value={sum.length - stale.length - failed.length} /></Card></Col>
      <Col span={6}><Card><Statistic title="超过 24h 未备份" value={stale.length} valueStyle={{ color: stale.length ? '#faad14' : undefined }} /></Card></Col>
      <Col span={6}><Card><Statistic title="最近失败" value={failed.length} valueStyle={{ color: failed.length ? '#ff4d4f' : undefined }} /></Card></Col>
    </Row>
    <Card size="small" title={`备份存储对象（${objects?.provider || 's3/minio'}，共 ${objects?.objects.length || 0} 个）`}
      style={{ marginTop: 16 }}>
      {objects?.objects.length
        ? <ul style={{ maxHeight: 160, overflow: 'auto', margin: 0, paddingLeft: 18 }}>
            {objects.objects.map((o) => <li key={o} style={{ fontFamily: 'monospace', fontSize: 12 }}>{o}</li>)}
          </ul>
        : <span style={{ color: '#94a3b8' }}>无对象（gateway.yaml storage 段未配置 S3/MinIO，或桶为空）</span>}
    </Card>
    <ProTable<BackupStatusRow> rowKey="id" loading={isLoading} search={false}
      style={{ marginTop: 16 }}
      dataSource={statuses?.backups || []} headerTitle="备份状态记录（备份 agent 通过 POST /api/backups/status 上报）"
      columns={[
        { title: '数据库', dataIndex: 'db_name' },
        { title: '目标', dataIndex: 'target', ellipsis: true, render: (_, r) => r.target || '-' },
        { title: '状态', dataIndex: 'status', render: (_, r) => <Tag color={statusColor[r.status]}>{r.status}</Tag> },
        { title: '大小', dataIndex: 'size_bytes', render: (_, r) => fmtSize(r.size_bytes) },
        { title: '消息', dataIndex: 'message', ellipsis: true, render: (_, r) => r.message || '-' },
        { title: '开始', dataIndex: 'started_at', width: 160 },
        { title: '结束', dataIndex: 'finished_at', width: 160, render: (_, r) => r.finished_at || '-' },
      ]} />
  </>;
}
