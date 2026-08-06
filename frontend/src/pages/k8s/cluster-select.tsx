import { Select } from 'antd';
import { useQuery } from '@tanstack/react-query';
import { k8sApi } from '../../services/k8s';

export function useClusterPicker() {
  const { data } = useQuery({ queryKey: ['clusters'], queryFn: () => k8sApi.listClusters() });
  const options = (data?.clusters || []).map((c) => ({ value: c.id, label: `${c.name} (${c.id})` }));
  return { options, defaultCluster: data?.clusters?.[0]?.id };
}

export function ClusterPicker({ value, onChange }: { value?: string; onChange: (v: string) => void }) {
  const { options } = useClusterPicker();
  return <Select allowClear showSearch placeholder="选择集群" style={{ width: 240, marginBottom: 12 }}
    value={value} onChange={(v) => onChange(v || '')} options={options}
    optionFilterProp="label" />;
}
