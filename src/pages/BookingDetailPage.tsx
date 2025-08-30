import React from 'react';
import { Card, Table, Tag } from 'antd';
import type { ColumnsType } from 'antd/es/table';

interface BookingRecord {
    key: string;
    date: string;
    time: string;
    status: 'pending' | 'approved' | 'rejected';
    purpose: string;
}

const BookingDetailPage: React.FC = () => {
    const columns: ColumnsType<BookingRecord> = [
        {
            title: '日期',
            dataIndex: 'date',
            key: 'date',
        },
        {
            title: '时间',
            dataIndex: 'time',
            key: 'time',
        },
        {
            title: '状态',
            dataIndex: 'status',
            key: 'status',
            render: (status: string) => {
                const statusMap = {
                    pending: { color: 'gold', text: '待审核' },
                    approved: { color: 'green', text: '已批准' },
                    rejected: { color: 'red', text: '已拒绝' },
                };
                const { color, text } = statusMap[status as keyof typeof statusMap];
                return <Tag color={color}>{text}</Tag>;
            },
        },
        {
            title: '用途',
            dataIndex: 'purpose',
            key: 'purpose',
        },
    ];

    const data: BookingRecord[] = [
        {
            key: '1',
            date: '2024-03-20',
            time: '09:00 - 11:00',
            status: 'approved',
            purpose: '系统测试',
        },
        {
            key: '2',
            date: '2024-03-21',
            time: '14:00 - 16:00',
            status: 'pending',
            purpose: '性能优化',
        },
    ];

    return (
        <Card title="预订详情">
            <Table columns={columns} dataSource={data} />
        </Card>
    );
};

export default BookingDetailPage; 