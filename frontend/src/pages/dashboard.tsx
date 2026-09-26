import { StatisticCard } from '@ant-design/pro-components';
import { Row, Col, Card, List } from 'antd';
import { CloudServerOutlined, ContainerOutlined, GithubOutlined, WarningOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { alertApi, cmdbApi } from '../services/api';
import { k8sApi } from '../services/k8s';
import SuperPet, { type PetState } from '../components/super-pet';

// 超猫状态 ← 后端熔断器 / 告警级别：CRIT* → alert，WARN* → degraded（见 docs/images/pet-states.svg）
const PET_TEXT: Record<PetState, string> = { ok: '一切正常', degraded: '降级运行', alert: '熔断 / 严重告警' };

export default function Dashboard() {
  const { data: clustersData } = useQuery({ queryKey: ['k8s-clusters'], queryFn: () => k8sApi.listClusters() });
  const { data: aggregateData } = useQuery({ queryKey: ['k8s-aggregate'], queryFn: () => k8sApi.aggregate() });
  const { data: statsData } = useQuery({ queryKey: ['cmdb-stats'], queryFn: () => cmdbApi.getCmdbStats() });
  const { data: alertsData } = useQuery({ queryKey: ['alerts'], queryFn: () => alertApi.listAlerts() });
  const { data: acksData } = useQuery({ queryKey: ['alert-acks'], queryFn: () => alertApi.listAcks() });
  const unacked = (alertsData?.alerts || []).filter((a) => !(acksData?.ids || []).includes(a.id));
  const petState: PetState = unacked.some((a) => a.level.toUpperCase().startsWith('CRIT'))
    ? 'alert' : unacked.length ? 'degraded' : 'ok';
  const totals = aggregateData?.totals;
  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col span={6}><StatisticCard statistic={{ title: 'K8s 集群', value: totals?.clusters ?? clustersData?.clusters.length ?? 0, icon: <CloudServerOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: 'Docker 主机', value: statsData?.hosts || 0, icon: <ContainerOutlined /> }} /></Col>
        {/* Pipeline 暂以 CMDB app 类资产数为口径，待审批流接入后替换 */}
        <Col span={6}><StatisticCard statistic={{ title: 'Pipeline', value: statsData?.app || 0, icon: <GithubOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: '活跃告警', value: unacked.length, icon: <WarningOutlined /> }} /></Col>
      </Row>
      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col span={12}><Card title="集群健康">
          {totals && <div style={{ marginBottom: 12, color: '#666' }}>
            节点 {totals.nodes_ready}/{totals.nodes} Ready · Pod {totals.pods_running}/{totals.pods} Running
          </div>}
          <List
            dataSource={aggregateData?.clusters || []}
            locale={{ emptyText: '暂无集群' }}
            renderItem={(c) => (
              <List.Item>
                <List.Item.Meta
                  title={`${c.name} (${c.status})`}
                  description={`节点 ${c.nodes_ready}/${c.node_count} Ready · Pod ${c.pods_running}/${c.pod_count} Running`}
                />
              </List.Item>
            )}
          />
        </Card></Col>
        <Col span={12}><Card title="超猫值守 · 最近告警">
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 8 }}>
            <SuperPet size={56} state={petState} />
            <div>
              <div style={{ fontWeight: 600 }}>{PET_TEXT[petState]}</div>
              <div style={{ color: '#666', fontSize: 12 }}>
                未确认告警 {unacked.length} 条 · 项圈 LED {
                  petState === 'ok' ? '绿（熔断器闭合）' : petState === 'degraded' ? '黄（降级/探针）' : '红（熔断打开）'}
              </div>
            </div>
          </div>
          <List dataSource={unacked.slice(0, 5)} locale={{ emptyText: '暂无未确认告警' }}
            renderItem={(a) => <List.Item><List.Item.Meta title={a.title || a.message} description={`${a.level} · ${a.message}`} /></List.Item>} />
        </Card></Col>
      </Row>
    </div>
  );
}
