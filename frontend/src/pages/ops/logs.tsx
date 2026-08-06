import { ProTable } from '@ant-design/pro-components';
import { Button, DatePicker, Input, Space } from 'antd';
import { SearchOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { logApi, LogRow, LogSearchParams } from '../../services/api';

export default function LogsPage() {
  const [namespace, setNamespace] = useState('');
  const [pod, setPod] = useState('');
  const [keyword, setKeyword] = useState('');
  const [range, setRange] = useState<[number, number] | null>(null);
  const [params, setParams] = useState<LogSearchParams | null>(null);

  const { data, isLoading, isFetching, error } = useQuery({
    queryKey: ['logs', params],
    queryFn: () => logApi.searchLogs(params as LogSearchParams),
    enabled: !!params,
  });

  const doSearch = () => {
    const next: LogSearchParams = { limit: 100 };
    if (namespace.trim()) next.namespace = namespace.trim();
    if (pod.trim()) next.pod = pod.trim();
    if (keyword.trim()) next.keyword = keyword.trim();
    if (range) { next.from = range[0]; next.to = range[1]; }
    setParams(next);
  };

  const columns = [
    { title: '时间', dataIndex: 'ts', width: 170 },
    { title: '命名空间', dataIndex: 'namespace', width: 130 },
    { title: 'Pod', dataIndex: 'pod', width: 160 },
    { title: '内容', dataIndex: 'content', ellipsis: true },
  ];

  return <>
    <Space style={{ marginBottom: 12, flexWrap: 'wrap' }} wrap>
      <Input placeholder="命名空间" value={namespace} onChange={e => setNamespace(e.target.value)}
        style={{ width: 160 }} onPressEnter={doSearch} />
      <Input placeholder="Pod" value={pod} onChange={e => setPod(e.target.value)}
        style={{ width: 160 }} onPressEnter={doSearch} />
      <Input placeholder="关键字（如 error）" value={keyword} onChange={e => setKeyword(e.target.value)}
        style={{ width: 220 }} onPressEnter={doSearch} />
      <DatePicker.RangePicker showTime
        onChange={(v) => setRange(v ? [v[0]!.unix(), v[1]!.unix()] : null)}
        placeholder={['起始时间', '结束时间']} />
      <Button type="primary" icon={<SearchOutlined />} onClick={doSearch} loading={isFetching}>搜索</Button>
    </Space>
    <ProTable<LogRow> rowKey={(r) => `${r.ts}-${r.namespace}-${r.pod}`} search={false}
      loading={isLoading}
      dataSource={data?.logs || []}
      columns={columns as never}
      headerTitle={params ? `日志检索结果（最多 ${params.limit || 100} 条）` : '日志检索'}
      locale={{ emptyText: error ? `查询失败：${error.message}` : '请输入条件后搜索' }}
      options={false}
      pagination={false} />
  </>;
}
