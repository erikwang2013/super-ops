import { ProTable } from '@ant-design/pro-components';
import { Select, Tag } from 'antd';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { opsApi, AuditEvent } from '../../services/api';

export default function AuditPage() {
  const [level, setLevel] = useState('');
  const { data, isLoading } = useQuery({
    queryKey: ['audit', level],
    queryFn: () => opsApi.getAuditEvents(100, 0, level),
  });
  return <ProTable<AuditEvent> rowKey={(r) => `${r.timestamp}_${r.event_type}_${r.username}`}
    columns={[
      { title: '时间', dataIndex: 'timestamp', width: 180 },
      { title: '用户', dataIndex: 'username', width: 120 },
      { title: '动作', dataIndex: 'event_type', width: 160, render: (_, r) => <Tag>{r.event_type}</Tag> },
      { title: '资源', dataIndex: 'ip', width: 140 },
      { title: '详情', dataIndex: 'detail', ellipsis: true },
    ]} dataSource={data?.events || []} loading={isLoading} search={false} headerTitle="审计中心"
    toolBarRender={() => [<Select key="level" value={level} style={{ width: 120 }} onChange={setLevel} options={[
      { value: '', label: '全部' }, { value: 'INFO', label: 'INFO' },
    ]} />]} />;
}
