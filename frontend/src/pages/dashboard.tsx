import { StatisticCard } from '@ant-design/pro-components';
import { Row, Col, Card, List } from 'antd';
import { CloudServerOutlined, ContainerOutlined, GithubOutlined, WarningOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { cmdbApi, opsApi } from '../services/api';
import { k8sApi } from '../services/k8s';

export default function Dashboard() {
  const { data: clustersData } = useQuery({ queryKey: ['k8s-clusters'], queryFn: () => k8sApi.listClusters() });
  const { data: aggregateData } = useQuery({ queryKey: ['k8s-aggregate'], queryFn: () => k8sApi.aggregate() });
  const { data: statsData } = useQuery({ queryKey: ['cmdb-stats'], queryFn: () => cmdbApi.getCmdbStats() });
  const { data: auditData } = useQuery({ queryKey: ['audit-events'], queryFn: () => opsApi.getAuditEvents(100, 0) });
  const totals = aggregateData?.totals;
  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col span={6}><StatisticCard statistic={{ title: 'K8s 集群', value: totals?.clusters ?? clustersData?.clusters.length ?? 0, icon: <CloudServerOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: 'Docker 主机', value: statsData?.hosts || 0, icon: <ContainerOutlined /> }} /></Col>
        {/* Pipeline 暂以 CMDB app 类资产数为口径，待审批流接入后替换 */}
        <Col span={6}><StatisticCard statistic={{ title: 'Pipeline', value: statsData?.app || 0, icon: <GithubOutlined /> }} /></Col>
        {/* 活跃告警暂以审计事件数为口径，P6 告警页就绪后替换为真实告警数 */}
        <Col span={6}><StatisticCard statistic={{ title: '活跃告警', value: auditData?.events.length || 0, icon: <WarningOutlined /> }} /></Col>
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
        <Col span={12}><Card title="最近告警"><List dataSource={[]} locale={{ emptyText: '暂无告警' }} renderItem={(i: any) => <List.Item><List.Item.Meta title={i.message} description={i.time} /></List.Item>} /></Card></Col>
      </Row>
    </div>
  );
}
