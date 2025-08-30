import React from 'react';
import { Card, Form, Input, DatePicker, Button, message } from 'antd';
import { useNavigate } from 'react-router-dom';

const { RangePicker } = DatePicker;

const BookingPage: React.FC = () => {
    const navigate = useNavigate();
    const [form] = Form.useForm();

    const onFinish = (values: any) => {
        console.log('Success:', values);
        message.success('预订成功！');
        navigate('/booking-detail');
    };

    return (
        <Card title="机时预订" style={{ maxWidth: 800, margin: '0 auto' }}>
            <Form
                form={form}
                layout="vertical"
                onFinish={onFinish}
                initialValues={{ remember: true }}
            >
                <Form.Item
                    label="预订时间"
                    name="bookingTime"
                    rules={[{ required: true, message: '请选择预订时间！' }]}
                >
                    <RangePicker showTime format="YYYY-MM-DD HH:mm" />
                </Form.Item>

                <Form.Item
                    label="用途说明"
                    name="purpose"
                    rules={[{ required: true, message: '请输入用途说明！' }]}
                >
                    <Input.TextArea rows={4} />
                </Form.Item>

                <Form.Item>
                    <Button type="primary" htmlType="submit">
                        提交预订
                    </Button>
                </Form.Item>
            </Form>
        </Card>
    );
};

export default BookingPage; 