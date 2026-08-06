import { ProTable } from '@ant-design/pro-components';
import { Button, Select, Tag, message } from 'antd';
import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { alertApi, AlertRow } from '../../services/api';

const LEVEL_COLOR: Record<string, string> = {
  CRIT: 'red', critical: 'red',
  WARN: 'orange', warning: 'orange',
  INFO: 'blue', info: 'blue',
};

export default function AlertsPage() {
  const [level, setLevel] = useState('');
  const qc = useQueryClient();
  const { data, isLoading } = useQuery({
    queryKey: ['alerts', level],
    queryFn: () => alertApi.listAlerts(level || undefined),
    refetchInterval: 10_000,
  });
  const { data: acksData } = useQuery({
    queryKey: ['alert-acks'],
    queryFn: () => alertApi.listAcks(),
    refetchInterval: 10_000,
  });
  const acked = new Set(acksData?.ids || []);
  const ack = useMutation({
    mutationFn: (id: number) => alertApi.ackAlert(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['alert-acks'] });
      message.success('已确认');
    },
    onError: (e: Error) => message.error(e.message),
  });
  return <ProTable<AlertRow> rowKey="id" search={false} loading={isLoading}
    dataSource={data?.alerts || []} headerTitle="告警中心" pagination={{ pageSize: 20 }}
    onRow={(r) => ({ style: acked.has(r.id) ? { opacity: 0.5 } : {} })}
    columns={[
      { title: '时间', dataIndex: 'id', width: 180, render: (_, r) => new Date(r.id * 1000).toLocaleString() },
      { title: '级别', dataIndex: 'level', width: 90, render: (_, r) => <Tag color={LEVEL_COLOR[r.level] || 'default'}>{r.level}</Tag> },
      { title: '标题', dataIndex: 'title', width: 180 },
      { title: '消息', dataIndex: 'message', ellipsis: true },
      { title: '操作', width: 90, render: (_, r) => acked.has(r.id)
        ? <span style={{ color: '#999' }}>已确认</span>
        : <Button size="small" type="primary" onClick={() => ack.mutate(r.id)}>确认</Button> },
    ]}
    toolBarRender={() => [<Select key="level" value={level} style={{ width: 130 }} onChange={setLevel} options={[
      { value: '', label: '全部' }, { value: 'info', label: 'info' },
      { value: 'warning', label: 'warning' }, { value: 'critical', label: 'critical' },
    ]} />]} />;
}
