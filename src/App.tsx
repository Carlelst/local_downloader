import React, { useState } from 'react';
import { Layout, Menu } from 'antd';
import { BrowserRouter as Router, Route, Routes, Link, useLocation } from 'react-router-dom';
import BookingPage from './pages/BookingPage';
import BookingDetailPage from './pages/BookingDetailPage';
import OtherPage from './pages/OtherPage';
import './App.css';

const { Sider, Content } = Layout;

const App: React.FC = () => {
    const [collapsed, setCollapsed] = useState(false);
    const location = useLocation();

    const menuItems = [
        {
            key: 'zebu',
            label: 'ZEBU 预订',
            children: [
                {
                    key: '/booking',
                    label: <Link to="/booking">预订</Link>,
                },
                {
                    key: '/booking-detail',
                    label: <Link to="/booking-detail">预订详情</Link>,
                },
            ],
        },
        {
            key: 'other',
            label: <Link to="/other">其他</Link>,
        },
    ];

    return (
        <Router>
            <Layout style={{ minHeight: '100vh' }}>
                <Sider
                    collapsible
                    collapsed={collapsed}
                    onCollapse={(value) => setCollapsed(value)}
                    theme="light"
                >
                    <div style={{ height: 32, margin: 16, background: 'rgba(0, 0, 0, 0.2)' }} />
                    <Menu
                        theme="light"
                        defaultSelectedKeys={[location.pathname]}
                        mode="inline"
                        items={menuItems}
                    />
                </Sider>
                <Layout>
                    <Content style={{ margin: '16px' }}>
                        <div style={{ padding: 24, minHeight: 360, background: '#fff' }}>
                            <Routes>
                                <Route path="/booking" element={<BookingPage />} />
                                <Route path="/booking-detail" element={<BookingDetailPage />} />
                                <Route path="/other" element={<OtherPage />} />
                            </Routes>
                        </div>
                    </Content>
                </Layout>
            </Layout>
        </Router>
    );
};

export default App; 