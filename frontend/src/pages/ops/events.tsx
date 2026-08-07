import { ProTable } from '@ant-design/pro-components';
import { Tag } from 'antd';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { DomainEventRow, eventApi } from '../../services/api';

const LEVEL_COLORS: Record<string, string> = {
  INFO: 'blue', WARN: 'orange', ERROR: 'red', FATAL: 'red',
};

const EVENT_TYPES = [
  { label: '告警', value: 'alert' },
  { label: '漂移', value: 'drift' },
  { label: '回滚', value: 'rollback' },
];

export default function EventsPage() {
  const [eventType, setEventType] = useState<string | undefined>();
  const { data, isLoading } = useQuery({
    queryKey: ['domain-events', eventType],
    queryFn: () => eventApi.list(100, eventType),
  });
  return <ProTable<DomainEventRow> rowKey={(r) => `${r.ts}-${r.event_type}-${r.title}`}
    search={false} loading={isLoading} dataSource={data?.events || []}
    headerTitle="领域事件（collector 告警/漂移/回滚经 ecat-events 事件总线发布，gateway 落 ClickHouse domain_event）"
    columns={[
      { title: '类型', dataIndex: 'event_type', width: 110, render: (_, r) => <Tag>{r.event_type}</Tag> },
      { title: '级别', dataIndex: 'level', width: 90, render: (_, r) => <Tag color={LEVEL_COLORS[r.level] || 'default'}>{r.level}</Tag> },
      { title: '标题', dataIndex: 'title', width: 180 },
      { title: '消息', dataIndex: 'message', ellipsis: true },
      { title: '时间', dataIndex: 'ts', width: 170 },
    ]}
    toolbar={{
      actions: [
        <select key="ft" value={eventType || ''} onChange={(e) => setEventType(e.target.value || undefined)}
          style={{ height: 30, borderRadius: 6, border: '1px solid #d9d9d9', padding: '0 8px' }}>
          <option value="">全部类型</option>
          {EVENT_TYPES.map((t) => <option key={t.value} value={t.value}>{t.label}</option>)}
        </select>,
      ],
    }} />;
}
