import { StatisticCard } from '@ant-design/pro-components';
import { Row, Col, Card, List } from 'antd';
import { CloudServerOutlined, ContainerOutlined, GithubOutlined, WarningOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { cmdbApi, opsApi } from '../services/api';
import { k8sApi } from '../services/k8s';

export default function Dashboard() {
  const { data: clustersData } = useQuery({ queryKey: ['k8s-clusters'], queryFn: () => k8sApi.listClusters() });
  const { data: statsData } = useQuery({ queryKey: ['cmdb-stats'], queryFn: () => cmdbApi.getCmdbStats() });
  const { data: auditData } = useQuery({ queryKey: ['audit-events'], queryFn: () => opsApi.getAuditEvents(100, 0) });
  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col span={6}><StatisticCard statistic={{ title: 'K8s 集群', value: clustersData?.clusters.length || 0, icon: <CloudServerOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: 'Docker 主机', value: statsData?.hosts || 0, icon: <ContainerOutlined /> }} /></Col>
        {/* Pipeline 暂以 CMDB app 类资产数为口径，待审批流接入后替换 */}
        <Col span={6}><StatisticCard statistic={{ title: 'Pipeline', value: statsData?.app || 0, icon: <GithubOutlined /> }} /></Col>
        {/* 活跃告警暂以审计事件数为口径，P6 告警页就绪后替换为真实告警数 */}
        <Col span={6}><StatisticCard statistic={{ title: '活跃告警', value: auditData?.events.length || 0, icon: <WarningOutlined /> }} /></Col>
      </Row>
      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col span={12}><Card title="集群健康"><List dataSource={[]} locale={{ emptyText: '暂无集群' }} renderItem={(i: any) => <List.Item>{i.name}</List.Item>} /></Card></Col>
        <Col span={12}><Card title="最近告警"><List dataSource={[]} locale={{ emptyText: '暂无告警' }} renderItem={(i: any) => <List.Item><List.Item.Meta title={i.message} description={i.time} /></List.Item>} /></Card></Col>
      </Row>
    </div>
  );
}
